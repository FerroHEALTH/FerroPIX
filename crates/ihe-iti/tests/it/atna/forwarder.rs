// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The forwarder: every message is stored first and delivered from the
//! spool in order; a repository that cannot be reached, or that stops
//! reading, leaves the messages stored and is retried within the timeouts
//! (ITI TF-2 §3.20.4.1.1); and a spooled message that cannot be sent is
//! quarantined so the drain goes on.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ihe_iti::atna::forwarder::Forwarder;
use ihe_iti::atna::repository::{Repository, Timeouts, TlsSettings};
use ihe_iti::atna::spool::{Bounds, QUARANTINE, Spool};
use tokio::io::AsyncReadExt as _;
use tokio::net::{TcpListener, TcpStream};
use url::Url;

use super::{message, until};
use crate::timing;

/// The RFC 5425 frame of `text`: its octet count, a space, and `text`.
fn framed(text: &str) -> secrecy::SecretSlice<u8> {
    message(&format!("{} {text}", text.len()))
}

const BOUNDS: Bounds = Bounds {
    max_messages: 16,
    max_bytes: 64 << 20,
    write_timeout: Duration::from_secs(10),
};

const TIMEOUTS: Timeouts = Timeouts {
    connect: Duration::from_secs(2),
    send: Duration::from_secs(2),
};

/// The longest one delivery attempt may take under [`TIMEOUTS`].
const ATTEMPT: Duration = TIMEOUTS.connect.saturating_add(TIMEOUTS.send);

/// The timeouts towards a repository that stops reading. The send timeout is
/// the slack a loaded host gets, so a store held to it tolerates the host
/// and still fails if it sits out a stalled write.
const STALLING: Timeouts = Timeouts {
    connect: TIMEOUTS.connect,
    send: timing::SLACK,
};

const RETRY_MAX: Duration = Duration::from_millis(400);

/// A plain TCP repository on a free loopback port. While `reading` is set it
/// keeps the octet-counted messages it reads; otherwise it accepts each
/// connection and never reads from it, as a stalled repository does.
async fn repository(reading: bool) -> (Url, Arc<Mutex<Vec<String>>>, Arc<AtomicBool>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a listener");
    let address = listener.local_addr().expect("an address");
    let kept = Arc::new(Mutex::new(Vec::new()));
    let switch = Arc::new(AtomicBool::new(reading));
    let (sink, reads) = (Arc::clone(&kept), Arc::clone(&switch));
    tokio::spawn(async move {
        let mut stalled: Vec<TcpStream> = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            if !reads.load(Ordering::SeqCst) {
                stalled.push(stream);
                continue;
            }
            tokio::spawn(read_frames(stream, Arc::clone(&sink)));
        }
    });
    let url = Url::parse(&format!("tcp://{address}")).expect("a URL");
    (url, kept, switch)
}

/// Reads frames off `stream` into `sink` as they arrive.
async fn read_frames(mut stream: TcpStream, sink: Arc<Mutex<Vec<String>>>) {
    let mut bytes = Vec::new();
    let mut chunk = vec![0_u8; 64 * 1024];
    while let Ok(read) = stream.read(&mut chunk).await {
        if read == 0 {
            return;
        }
        bytes.extend_from_slice(&chunk[..read]);
        while let Some(space) = bytes.iter().position(|byte| *byte == b' ') {
            let length: usize = std::str::from_utf8(&bytes[..space])
                .expect("a length")
                .parse()
                .expect("a number");
            let end = space + 1 + length;
            if bytes.len() < end {
                break;
            }
            let text = String::from_utf8_lossy(&bytes[space + 1..end]).into_owned();
            sink.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(text);
            bytes.drain(..end);
        }
    }
}

fn kept_now(kept: &Mutex<Vec<String>>) -> Vec<String> {
    kept.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Waits until the repository has kept `count` messages or `deadline`
/// passes, and returns what it kept.
async fn kept_by(kept: &Mutex<Vec<String>>, count: usize, deadline: Instant) -> Vec<String> {
    loop {
        let now = kept_now(kept);
        if now.len() >= count || Instant::now() >= deadline {
            return now;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn stored_messages_are_delivered_in_order_and_leave_the_spool() {
    let (url, kept, _) = repository(true).await;
    let repository = Repository::unencrypted_for_development(&url, TIMEOUTS).expect("a repository");
    let forwarder = Forwarder::new(Spool::in_memory(BOUNDS), repository, RETRY_MAX);
    for text in ["one", "two", "three"] {
        forwarder.submit(framed(text)).await.expect("stored");
    }
    let running = tokio::spawn(Arc::clone(&forwarder).run());
    let status = until(&forwarder, timing::deadline(ATTEMPT), |status| {
        status.delivered == 3
    })
    .await;
    assert_eq!(3, status.delivered);
    assert_eq!(0, status.depth.messages);
    assert!(status.reachable);
    let received = kept_by(&kept, 3, timing::deadline(Duration::ZERO)).await;
    running.abort();
    assert_eq!(vec!["one", "two", "three"], received);
}

#[tokio::test]
async fn a_repository_that_cannot_be_reached_leaves_every_message_stored() {
    let url = Url::parse("tcp://127.0.0.1:0").expect("a URL");
    let repository = Repository::unencrypted_for_development(&url, TIMEOUTS).expect("a repository");
    let forwarder = Forwarder::new(Spool::in_memory(BOUNDS), repository, RETRY_MAX);
    let running = tokio::spawn(Arc::clone(&forwarder).run());
    for text in ["kept", "also kept"] {
        forwarder.submit(framed(text)).await.expect("stored");
    }
    let status = until(
        &forwarder,
        timing::deadline(ATTEMPT.saturating_add(RETRY_MAX).saturating_mul(2)),
        |status| status.retries >= 2,
    )
    .await;
    running.abort();
    assert!(!status.reachable);
    assert!(status.retries >= 2, "every failed attempt is counted");
    assert_eq!(0, status.delivered);
    assert_eq!(2, status.depth.messages, "nothing is dropped");
}

#[tokio::test]
async fn a_repository_that_stops_reading_is_given_up_within_the_send_timeout() {
    let (url, kept, reading) = repository(false).await;
    let repository = Repository::unencrypted_for_development(&url, STALLING).expect("a repository");
    let forwarder = Forwarder::new(Spool::in_memory(BOUNDS), repository, RETRY_MAX);
    // NOTE: a message larger than the socket buffers, so the write blocks
    // on a repository that never reads.
    let large = "x".repeat(16 << 20);
    forwarder.submit(framed(&large)).await.expect("stored");
    let running = tokio::spawn(Arc::clone(&forwarder).run());

    let given_up_by = timing::deadline(STALLING.connect.saturating_add(STALLING.send));
    let stalled = until(&forwarder, given_up_by, |status| status.retries >= 1).await;
    assert!(
        stalled.retries >= 1,
        "the stalled write ends within the send timeout"
    );
    assert!(!stalled.reachable);
    assert_eq!(1, stalled.depth.messages, "the message stays spooled");

    let submitted = Instant::now();
    forwarder.submit(framed("after")).await.expect("stored");
    assert!(
        submitted.elapsed() < STALLING.send,
        "storing never waits on the network"
    );

    reading.store(true, Ordering::SeqCst);
    // The forwarder may be in a stalled write: it gives that up, backs off,
    // reconnects, and writes both messages, each within the send timeout.
    let recovery = STALLING
        .send
        .saturating_add(RETRY_MAX)
        .saturating_add(STALLING.connect)
        .saturating_add(STALLING.send.saturating_mul(2));
    let recovered = until(&forwarder, timing::deadline(recovery), |status| {
        status.delivered == 2
    })
    .await;
    assert_eq!(
        2, recovered.delivered,
        "delivered once the repository reads again"
    );
    assert_eq!(0, recovered.depth.messages);
    let received = kept_by(&kept, 2, timing::deadline(Duration::ZERO)).await;
    running.abort();
    assert_eq!(Some("after"), received.get(1).map(String::as_str));
}

#[tokio::test]
async fn a_spooled_message_that_cannot_be_sent_is_quarantined_and_the_rest_delivered() {
    let (url, kept, _) = repository(true).await;
    let directory = tempfile::tempdir().expect("a directory");
    let path = directory.path().join("spool");
    {
        let spool = Spool::open(&path, BOUNDS).expect("the spool opens");
        for text in ["first", "second", "third"] {
            spool.push(framed(text)).await.expect("stored");
        }
    }
    let first = path.join(format!("{:020}.msg", 0));
    std::fs::write(&first, b"not a syslog frame").expect("a corrupt message");

    let repository = Repository::unencrypted_for_development(&url, TIMEOUTS).expect("a repository");
    let spool = Spool::open(&path, BOUNDS).expect("the spool reopens");
    let forwarder = Forwarder::new(spool, repository, RETRY_MAX);
    let running = tokio::spawn(Arc::clone(&forwarder).run());
    let status = until(&forwarder, timing::deadline(ATTEMPT), |status| {
        status.delivered == 2
    })
    .await;
    let received = kept_by(&kept, 2, timing::deadline(Duration::ZERO)).await;
    running.abort();

    assert_eq!(2, status.delivered, "one bad file never blocks the rest");
    assert_eq!(1, status.depth.quarantined);
    assert_eq!(
        1, status.depth.messages,
        "the quarantined message stays counted"
    );
    assert_eq!(vec!["second", "third"], received);
    assert!(
        path.join(QUARANTINE)
            .join(format!("{:020}.msg", 0))
            .is_file()
    );
    assert!(!first.exists());
}

#[test]
fn an_address_of_another_form_is_refused() {
    for address in [
        "https://arr.example.org",
        "tls://arr.example.org/path",
        "tls://user@arr.example.org",
    ] {
        let url = Url::parse(address).expect("a URL");
        assert!(
            Repository::tls(&url, &TlsSettings::default(), TIMEOUTS).is_err(),
            "{address}"
        );
    }
    let plain = Url::parse("tcp://arr.example.org").expect("a URL");
    assert!(
        Repository::unencrypted_for_development(&plain, TIMEOUTS).is_err(),
        "a plain repository names its port"
    );
    let tls = Url::parse("tcp://arr.example.org:601").expect("a URL");
    assert!(
        Repository::tls(&tls, &TlsSettings::default(), TIMEOUTS).is_err(),
        "tls:// only"
    );
}
