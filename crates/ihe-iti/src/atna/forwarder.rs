// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The store-and-forward sender of ITI-20 messages (ITI TF-2 §3.20.4.1.1).
//!
//! Every message is stored in the [`Spool`] first and delivered from it,
//! oldest first, so a message counts as recorded once it is stored, and a
//! repository that cannot be reached delays its delivery without losing it.
//! [`Forwarder::submit`] only ever writes to the spool: it never waits on
//! the network, so a slow repository never holds an exchange, and it waits
//! on the disk at most the spool's [`Bounds::write_timeout`](super::spool::Bounds::write_timeout).
//! A message stored after its exchange stopped waiting for it is delivered
//! like any other.
//!
//! [`Forwarder::run`] is the delivery loop. To a syslog repository it keeps
//! one connection open, writes each stored message to it within the
//! repository's timeouts, removes the message once it is written, and after
//! any transport failure drops the connection and retries with an
//! exponential backoff, with jitter, capped at the configured longest wait.
//! To a FHIR Feed repository (feature `balp`) it posts each stored record,
//! removes it on a `2xx`, retries the same way after a failure that may
//! pass, and quarantines a record the repository refused, so the drain goes
//! on.

use std::hash::BuildHasher as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use secrecy::SecretSlice;

use super::chain;
#[cfg(feature = "balp")]
use super::feed::{FeedError, FeedRepository};
use super::repository::{Connection, Repository};
use super::spool::{Depth, Spool, SpoolError, Stored};

/// The first wait after a failed delivery.
const FIRST_RETRY: Duration = Duration::from_millis(250);

/// What the forwarder reports of itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// What the spool holds, awaiting delivery or in quarantine.
    pub depth: Depth,
    /// The messages delivered since the forwarder started.
    pub delivered: u64,
    /// The failed delivery attempts since the forwarder started, each
    /// followed by a backoff.
    pub retries: u64,
    /// Whether the last attempt to deliver succeeded; `false` while the
    /// forwarder retries.
    pub reachable: bool,
    /// Whether the spool is on disk.
    pub durable: bool,
}

/// Where a forwarder delivers to.
#[derive(Debug)]
enum Destination {
    /// A syslog repository (ITI TF-2 §3.20.4.1).
    Syslog(Repository),
    /// A FHIR Feed repository (ITI TF-2 §3.20.4.2).
    #[cfg(feature = "balp")]
    Feed(FeedRepository),
}

/// The store-and-forward sender to one Audit Record Repository.
#[derive(Debug)]
pub struct Forwarder {
    spool: Spool,
    destination: Destination,
    retry_max: Duration,
    delivered: AtomicU64,
    retries: AtomicU64,
    reachable: AtomicBool,
}

impl Forwarder {
    /// The sender of the syslog frames in `spool` to `repository`, waiting
    /// at most `retry_max` between two attempts.
    #[must_use]
    pub fn new(spool: Spool, repository: Repository, retry_max: Duration) -> Arc<Self> {
        Self::to(spool, Destination::Syslog(repository), retry_max)
    }

    /// The sender of the `AuditEvent` records in `spool` to the FHIR Feed
    /// repository `feed`, waiting at most `retry_max` between two attempts.
    #[cfg(feature = "balp")]
    #[must_use]
    pub fn fhir_feed(spool: Spool, feed: FeedRepository, retry_max: Duration) -> Arc<Self> {
        Self::to(spool, Destination::Feed(feed), retry_max)
    }

    fn to(spool: Spool, destination: Destination, retry_max: Duration) -> Arc<Self> {
        Arc::new(Self {
            spool,
            destination,
            retry_max: retry_max.max(FIRST_RETRY),
            delivered: AtomicU64::new(0),
            retries: AtomicU64::new(0),
            reachable: AtomicBool::new(true),
        })
    }

    /// Stores `frame` for delivery, returning once it is stored.
    ///
    /// # Errors
    ///
    /// The [`SpoolError`] of a spool that is full or cannot be written: the
    /// message is then neither stored nor delivered. Past the spool's
    /// [`Bounds::write_timeout`](super::spool::Bounds::write_timeout),
    /// [`SpoolError::Late`]: the message is not stored yet, and is delivered
    /// once its write stores it.
    pub async fn submit(&self, frame: SecretSlice<u8>) -> Result<(), SpoolError> {
        self.spool.push(frame).await
    }

    /// What the forwarder holds and how its deliveries went.
    #[must_use]
    pub fn status(&self) -> Status {
        Status {
            depth: self.spool.depth(),
            delivered: self.delivered.load(Ordering::Relaxed),
            retries: self.retries.load(Ordering::Relaxed),
            reachable: self.reachable.load(Ordering::Relaxed),
            durable: self.spool.is_durable(),
        }
    }

    /// Delivers the stored messages, oldest first, for as long as it runs.
    ///
    /// The caller spawns it once and aborts the task to stop it; a message
    /// in flight then stays stored and is delivered again by the next run.
    pub async fn run(self: Arc<Self>) {
        match &self.destination {
            Destination::Syslog(repository) => self.run_syslog(repository).await,
            #[cfg(feature = "balp")]
            Destination::Feed(feed) => self.run_feed(feed).await,
        }
    }

    /// The oldest stored message, after waiting for one while the spool is
    /// empty; `None` after a wake-up or after a failure to read the spool,
    /// which has backed off.
    async fn next(&self, retry: &mut Duration) -> Option<Stored> {
        match self.spool.oldest().await {
            Ok(Some(stored)) => Some(stored),
            Ok(None) => {
                self.spool.stored().await;
                None
            }
            Err(error) => {
                tracing::warn!(error = %chain(&error), "the audit spool could not be read");
                *retry = self.back_off(*retry).await;
                None
            }
        }
    }

    /// Removes the delivered message `sequence` and counts it.
    async fn delivered(&self, sequence: u64, retry: &mut Duration) {
        self.reachable.store(true, Ordering::Relaxed);
        *retry = FIRST_RETRY;
        match self.spool.remove(sequence).await {
            Ok(()) => {
                self.delivered.fetch_add(1, Ordering::Relaxed);
            }
            Err(error) => {
                tracing::warn!(
                    error = %chain(&error),
                    "a delivered audit message could not be removed from the spool"
                );
                *retry = self.back_off(*retry).await;
            }
        }
    }

    // NOTE: RFC 5425 §4.3 carries no application-level acknowledgement, so a
    // syslog repository never refuses a frame: only a transport failure retries.
    async fn run_syslog(&self, repository: &Repository) {
        let mut connection: Option<Connection> = None;
        let mut retry = FIRST_RETRY;
        loop {
            let Some(stored) = self.next(&mut retry).await else {
                continue;
            };
            let reusable = match connection.take() {
                Some(mut open) => (!open.is_closed().await).then_some(open),
                None => None,
            };
            let mut open = match reusable {
                Some(open) => open,
                None => match repository.connect().await {
                    Ok(open) => open,
                    Err(error) => {
                        tracing::warn!(
                            error = %chain(&error),
                            "the audit repository could not be reached; its messages stay spooled"
                        );
                        retry = self.back_off(retry).await;
                        continue;
                    }
                },
            };
            if let Err(error) = open.send(&stored.message).await {
                tracing::warn!(
                    error = %chain(&error),
                    "an audit message could not be delivered; the connection is dropped and the message stays spooled"
                );
                retry = self.back_off(retry).await;
                continue;
            }
            connection = Some(open);
            self.delivered(stored.sequence, &mut retry).await;
        }
    }

    // NOTE: the `RESTful` ATNA supplement §3.20.4.3.3 leaves a failure to the
    // client: a refusal is quarantined, every other failure is retried.
    #[cfg(feature = "balp")]
    async fn run_feed(&self, feed: &FeedRepository) {
        let mut retry = FIRST_RETRY;
        loop {
            let Some(stored) = self.next(&mut retry).await else {
                continue;
            };
            match feed.send(&stored.message).await {
                Ok(()) => self.delivered(stored.sequence, &mut retry).await,
                Err(FeedError::Rejected { status }) => {
                    self.reachable.store(true, Ordering::Relaxed);
                    tracing::error!(
                        sequence = stored.sequence,
                        status = status.as_u16(),
                        "the audit repository refused an AuditEvent; it is quarantined"
                    );
                    if let Err(error) = self.spool.reject(stored.sequence).await {
                        tracing::warn!(
                            error = %chain(&error),
                            "a refused audit message could not be quarantined"
                        );
                        retry = self.back_off(retry).await;
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        error = %chain(&error),
                        "an AuditEvent could not be delivered; it stays spooled"
                    );
                    retry = self.back_off(retry).await;
                }
            }
        }
    }

    /// Records a failed attempt, waits about `retry`, and returns the wait
    /// after it: twice as long, at most the configured longest wait.
    async fn back_off(&self, retry: Duration) -> Duration {
        self.reachable.store(false, Ordering::Relaxed);
        self.retries.fetch_add(1, Ordering::Relaxed);
        tokio::time::sleep(jittered(retry)).await;
        retry.saturating_mul(2).min(self.retry_max)
    }
}

/// A wait between half of `retry` and `retry`, so senders that failed
/// together do not retry together (no specification governs this: our own
/// design).
fn jittered(retry: Duration) -> Duration {
    let half = retry / 2;
    let spread = u64::try_from(half.as_nanos()).unwrap_or(u64::MAX);
    let random = std::collections::hash_map::RandomState::new().hash_one(Instant::now());
    half.saturating_add(Duration::from_nanos(random % spread.saturating_add(1)))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use secrecy::ExposeSecret as _;
    use tokio::io::AsyncReadExt as _;
    use tokio::net::TcpListener;
    use url::Url;

    use super::super::repository::{Repository, Timeouts};
    use super::super::spool::in_flight::tests::{
        BOUND, Logs, PATIENT, SLACK, Stall, naming_the_patient,
    };
    use super::super::spool::{Bounds, Spool, SpoolError};
    use super::Forwarder;

    /// ITI TF-2 §3.20.4.1.1: a message its exchange gave up on, stored once
    /// the stalled write ends, reaches the repository like any other.
    #[tokio::test]
    async fn a_message_stored_after_its_exchange_gave_up_is_delivered_once() {
        let logs = Logs::default();
        let _logging = tracing::subscriber::set_default(logs.subscriber());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a listener");
        let url = Url::parse(&format!(
            "tcp://{}",
            listener.local_addr().expect("an address")
        ))
        .expect("a URL");
        let timeouts = Timeouts {
            connect: SLACK,
            send: SLACK,
        };
        let repository =
            Repository::unencrypted_for_development(&url, timeouts).expect("a repository");
        let directory = tempfile::tempdir().expect("a directory");
        let bounds = Bounds {
            max_messages: 16,
            max_bytes: 1 << 20,
            write_timeout: BOUND,
        };
        let spool = Spool::open(&directory.path().join("spool"), bounds).expect("it opens");
        let forwarder = Forwarder::new(spool.clone(), repository, Duration::from_millis(400));
        let running = tokio::spawn(Arc::clone(&forwarder).run());
        // The forwarder finds the spool empty and waits for a stored message.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let stall = Stall::of(&spool);

        let asked = Instant::now();
        let refused = forwarder.submit(naming_the_patient()).await;
        assert!(asked.elapsed() < BOUND + SLACK, "{:?}", asked.elapsed());
        assert!(
            matches!(refused, Err(SpoolError::Late { .. })),
            "{refused:?}"
        );
        assert_eq!(
            0,
            forwarder.status().depth.messages,
            "not counted as stored"
        );

        stall.release();
        let (mut stream, _) = tokio::time::timeout(SLACK, listener.accept())
            .await
            .expect("the forwarder connects once the message is stored")
            .expect("a connection");
        let sent = naming_the_patient();
        let mut received = vec![0_u8; sent.expose_secret().len()];
        tokio::time::timeout(SLACK, stream.read_exact(&mut received))
            .await
            .expect("the message arrives")
            .expect("it reads");
        assert_eq!(sent.expose_secret(), received.as_slice());

        let until = Instant::now() + SLACK;
        while forwarder.status().delivered == 0 && Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let status = forwarder.status();
        assert_eq!(1, status.delivered, "delivered once");
        assert_eq!(0, status.depth.messages);
        running.abort();
        assert!(!logs.text().contains(PATIENT), "{}", logs.text());
    }
}
