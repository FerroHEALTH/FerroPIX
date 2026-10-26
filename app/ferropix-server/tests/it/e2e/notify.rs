// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-10 sent over PostgreSQL 18 (#113), behind `FERROPIX_E2E`:
//! - a link and a merge each write one notification per consumer whose
//!   domains of interest the person holds, and none for another;
//! - each consumer receives its notifications in order, over MLLP, and
//!   acknowledges each; each delivery is audited;
//! - an unreachable consumer is retried within the bounds and its failure is
//!   what the dependency report reads;
//! - no identifier or name reaches the log.
//!
//! Every name and identifier is synthetic.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the audit records are read as JSON values"
)]

use std::time::Duration;

use ferrobridge_hl7v2::ack::Code;
use ferropix_server::audit::Audit;
use ferropix_server::config::section::audit::AuditSettings;
use ferropix_server::config::section::hl7v2::{Consumer, Notification};
use ferropix_server::config::section::outbox::Outbox;
use ferropix_server::feed::Context;
use ferropix_server::feed::merging::{self, MergeRequest};
use ferropix_server::feed::notify::Notifying;
use ferropix_server::hl7v2::authority::Authorities;
use ferropix_server::outbox::{self, Backoff, Channels, MllpChannel, Wake};
use ferropix_server::store::Store;
use ferropix_server::store::outbox::last_notification;
use ferropix_server::store::record::{Channel, PersonId};
use ferropix_testkit::consumer::Consumer as Stub;
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{
    FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN,
};

use super::bsn;
use super::feeds::{Patient, feeder, start};
use super::leak::Capture;

/// A consumer named `name` at `address`, interested in `domains`.
fn consumer(name: &str, address: &str, domains: &[&str]) -> Consumer {
    Consumer {
        name: name.to_owned(),
        address: address.to_owned(),
        application: String::from("PIXCONSUMER"),
        facility: name.to_uppercase(),
        domains: domains.iter().map(|domain| (*domain).to_owned()).collect(),
        connect_timeout_ms: 1_000,
        reply_timeout_ms: 2_000,
    }
}

/// The retry bounds of every case: `attempts`, with short waits.
fn bounds(attempts: u32) -> Notification {
    Notification {
        max_attempts: attempts,
        initial_backoff_ms: 20,
        max_backoff_ms: 40,
    }
}

/// `store` notifying `consumers`, woken by `wake`.
fn notifying(store: &Store, consumers: &[Consumer], attempts: u32, wake: &Wake) -> Store {
    store.clone().with_consumers(Notifying {
        manager: (String::from("FERROPIX"), String::from("NORTH")),
        consumers: consumers.to_vec().into(),
        authorities: Authorities::default(),
        notification: bounds(attempts),
        wake: wake.clone(),
    })
}

/// An audit trail spooled under `directory`, with no repository.
fn audit(directory: &std::path::Path) -> Audit {
    std::fs::create_dir_all(directory).expect("the spool directory");
    std::fs::set_permissions(
        directory,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .expect("the spool is private");
    Audit::open(
        &AuditSettings {
            url: None,
            spool_dir: directory.to_owned(),
            spool_max_bytes: 1 << 20,
            spool_max_events: 50,
            spool_write_timeout: Duration::from_secs(2),
            timeout: Duration::from_secs(2),
            retry_max: Duration::from_millis(200),
        },
        &url::Url::parse("https://pix.example.org/fhir").expect("a URL"),
    )
    .expect("the spool opens")
}

/// Starts the sender over `store` delivering ITI-10 to `consumers`.
fn sender(
    store: &Store,
    consumers: &[Consumer],
    audit: &Audit,
    wake: &Wake,
) -> tokio::sync::watch::Sender<bool> {
    let channels = Channels {
        xcpd: None,
        pmir: None,
        hl7v2: Some(MllpChannel {
            consumers: consumers.to_vec().into(),
            authorities: Authorities::default(),
            frame_limit: 1 << 20,
            backoff: Backoff {
                initial: Duration::from_millis(20),
                max: Duration::from_millis(40),
            },
            audit: audit.clone(),
        }),
    };
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let retention = Outbox::default().resolve().expect("the default retention");
    let _sender = outbox::start(store.clone(), channels, retention, wake.clone(), stopped);
    stop
}

/// The outbox's ITI-10 rows as `(consumer, attempts, delivered, abandoned,
/// last failure)`, oldest first.
async fn rows(store: &Store) -> Vec<(String, i32, bool, bool, Option<String>)> {
    sqlx::query_as(
        "SELECT consumer, attempts, delivered_at IS NOT NULL, abandoned_at IS NOT NULL,
                last_failure
         FROM pix.outbox WHERE channel = 'hl7v2' ORDER BY created_at, id",
    )
    .fetch_all(store.pool())
    .await
    .expect("the outbox reads")
}

/// How many ITI-10 rows name `consumer`.
async fn written(store: &Store, consumer: &str) -> usize {
    rows(store)
        .await
        .iter()
        .filter(|row| row.0 == consumer)
        .count()
}

/// Waits until `done` holds of the ITI-10 rows.
async fn until(store: &Store, done: impl Fn(&[(String, i32, bool, bool, Option<String>)]) -> bool) {
    for _ in 0..400 {
        if done(&rows(store).await) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the outbox did not settle: {:?}", rows(store).await);
}

/// The PID-3 of `message`, as written.
fn pid_3(message: &str) -> String {
    message
        .split('\r')
        .find(|segment| segment.starts_with("PID|"))
        .and_then(|pid| pid.split('|').nth(3))
        .expect("a PID-3")
        .to_owned()
}

/// The values of the CX repetitions of `pid_3`.
fn values(pid_3: &str) -> Vec<String> {
    pid_3
        .split('~')
        .map(|cx| cx.split('^').next().unwrap_or_default().to_owned())
        .collect()
}

/// The ITI-10 audit records in the spool under `directory`.
fn audited(directory: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_dir(directory)
        .expect("the spool reads")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "msg"))
        .map(|entry| serde_json::from_slice(&std::fs::read(entry.path()).unwrap()).unwrap())
        .filter(|record: &serde_json::Value| record.to_string().contains("ITI-10"))
        .collect()
}

#[tokio::test]
async fn a_link_notifies_each_matching_consumer_once_in_order_and_acknowledged() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let hospital = Stub::start(Code::Accept).await.expect("a stub");
    let every = Stub::start(Code::Accept).await.expect("a stub");
    let practice = Stub::start(Code::Accept).await.expect("a stub");
    let consumers = [
        consumer("hospital", &hospital.address(), &[HOSPITAL_DOMAIN]),
        consumer("every", &every.address(), &[]),
        consumer("practice", &practice.address(), &[PRACTICE_DOMAIN]),
    ];
    let wake = Wake::default();
    let store = notifying(&started.store, &consumers, 3, &wake);
    let feeder = feeder(&store);
    let shared = bsn(60).expose().to_owned();
    let first = feeder
        .revise(
            &Patient::new(HOSPITAL_DOMAIN, "H-60", FAMILY_NAMES[1], GIVEN_NAMES[1])
                .carrying(super::BSN_SYSTEM, &shared)
                .revision(),
            &super::feeds::context(),
        )
        .await
        .expect("fed");
    assert_eq!(1, written(&store, "hospital").await);
    assert_eq!(1, written(&store, "every").await);
    let second = feeder
        .revise(
            &Patient::new(LABORATORY_DOMAIN, "L-60", FAMILY_NAMES[1], GIVEN_NAMES[1])
                .carrying(super::BSN_SYSTEM, &shared)
                .revision(),
            &super::feeds::context(),
        )
        .await
        .expect("fed");
    assert_eq!(
        first.person, second.person,
        "the laboratory record is linked"
    );
    assert_eq!(2, written(&store, "hospital").await, "one for the link");
    assert_eq!(2, written(&store, "every").await, "one for the link");
    assert_eq!(
        0,
        written(&store, "practice").await,
        "no practice identifier"
    );

    let spool = tempfile::tempdir().expect("a temporary directory");
    let directory = spool.path().join("spool");
    let trail = audit(&directory);
    let _stop = sender(&store, &consumers, &trail, &wake);
    until(&store, |rows| rows.iter().all(|row| row.2)).await;
    let to_hospital = hospital.received();
    assert_eq!(2, to_hospital.len());
    for message in &to_hospital {
        assert_eq!(vec![String::from("H-60")], values(&pid_3(message)));
        assert!(message.contains("|ADT^A31^ADT_A05|"));
    }
    let to_every = every.received();
    assert_eq!(2, to_every.len());
    assert_eq!(
        2,
        values(&pid_3(&to_every[0])).len(),
        "the BSN and H-60 first"
    );
    let mut linked = values(&pid_3(&to_every[1]));
    linked.sort();
    let mut wanted = vec![shared.clone(), String::from("H-60"), String::from("L-60")];
    wanted.sort();
    assert_eq!(wanted, linked, "then the linked set");
    assert_eq!(0, practice.received().len(), "nothing for the practice");
    assert_eq!(4, audited(&directory).len(), "one record per delivery");
}

#[tokio::test]
async fn a_steward_merge_notifies_each_matching_consumer_once_with_the_survivor() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let hospital = Stub::start(Code::Accept).await.expect("a stub");
    let laboratory = Stub::start(Code::Accept).await.expect("a stub");
    let consumers = [
        consumer("hospital", &hospital.address(), &[HOSPITAL_DOMAIN]),
        consumer("laboratory", &laboratory.address(), &[LABORATORY_DOMAIN]),
    ];
    let wake = Wake::default();
    let store = notifying(&started.store, &consumers, 3, &wake);
    let feeder = feeder(&store);
    let survivor = feeder
        .revise(
            &Patient::new(HOSPITAL_DOMAIN, "H-61", FAMILY_NAMES[2], GIVEN_NAMES[2]).revision(),
            &super::feeds::context(),
        )
        .await
        .expect("fed")
        .person
        .expect("a person");
    let merged = feeder
        .revise(
            &Patient::new(LABORATORY_DOMAIN, "L-61", FAMILY_NAMES[3], GIVEN_NAMES[3]).revision(),
            &super::feeds::context(),
        )
        .await
        .expect("fed")
        .person
        .expect("a person");
    assert_ne!(survivor, merged);
    assert_eq!(
        (1, 1),
        (
            written(&store, "hospital").await,
            written(&store, "laboratory").await
        )
    );
    let version = |person: PersonId| {
        let store = store.clone();
        async move {
            let mut connection = store.pool().acquire().await.expect("a connection");
            merging::version(&mut connection, person)
                .await
                .expect("reads")
                .expect("a person")
        }
    };
    let request = MergeRequest {
        survivor,
        survivor_version: version(survivor).await,
        merged,
        merged_version: version(merged).await,
        reason: String::from("one patient"),
    };
    let steward = Context {
        actor: String::from("synthetic-steward"),
        channel: Channel::Fhir,
        request_id: None,
    };
    let (transaction, _change) = merging::merge(&store, &request, &steward)
        .await
        .expect("merged");
    transaction.commit().await.expect("committed");
    assert_eq!(
        (2, 2),
        (
            written(&store, "hospital").await,
            written(&store, "laboratory").await
        ),
        "one notification each for the merge"
    );
    let spool = tempfile::tempdir().expect("a temporary directory");
    let trail = audit(&spool.path().join("spool"));
    let _stop = sender(&store, &consumers, &trail, &wake);
    until(&store, |rows| rows.iter().all(|row| row.2)).await;
    assert_eq!(
        vec![String::from("L-61")],
        values(&pid_3(&laboratory.received()[1])),
        "the survivor's identifiers in the laboratory domain"
    );
}

#[tokio::test]
async fn an_unreachable_consumer_is_retried_within_the_bounds_and_reported() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port");
    let address = closed.local_addr().expect("an address").to_string();
    drop(closed);
    let consumers = [consumer("gone", &address, &[])];
    let wake = Wake::default();
    let store = notifying(&started.store, &consumers, 2, &wake);
    feeder(&store)
        .revise(
            &Patient::new(HOSPITAL_DOMAIN, "H-62", FAMILY_NAMES[4], GIVEN_NAMES[4]).revision(),
            &super::feeds::context(),
        )
        .await
        .expect("fed");
    assert_eq!(
        None,
        last_notification(&store, "gone").await.expect("reads"),
        "nothing attempted yet"
    );
    let spool = tempfile::tempdir().expect("a temporary directory");
    let directory = spool.path().join("spool");
    let trail = audit(&directory);
    let _stop = sender(&store, &consumers, &trail, &wake);
    until(&store, |rows| rows.iter().all(|row| row.3)).await;
    let settled = rows(&store).await;
    assert_eq!(2, settled[0].1, "two attempts, the bound");
    assert_eq!(
        Some("the consumer refused the connection"),
        settled[0].4.as_deref()
    );
    let last = last_notification(&store, "gone")
        .await
        .expect("reads")
        .expect("attempted");
    assert_eq!(
        Err(String::from("the consumer refused the connection")),
        last.last,
        "the failure the dependency report shows"
    );
    assert!(
        audited(&directory).is_empty(),
        "nothing reached the consumer"
    );
}

#[tokio::test]
async fn no_identifier_or_name_of_a_notification_reaches_the_log() {
    if !e2e_enabled() {
        return;
    }
    let capture = Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());
    let started = start().await;
    let refusing = Stub::start(Code::Error).await.expect("a stub");
    let consumers = [consumer("refusing", &refusing.address(), &[])];
    let wake = Wake::default();
    let store = notifying(&started.store, &consumers, 2, &wake);
    let shared = bsn(63).expose().to_owned();
    feeder(&store)
        .revise(
            &Patient::new(HOSPITAL_DOMAIN, "H-63", FAMILY_NAMES[5], GIVEN_NAMES[5])
                .carrying(super::BSN_SYSTEM, &shared)
                .revision(),
            &super::feeds::context(),
        )
        .await
        .expect("fed");
    let spool = tempfile::tempdir().expect("a temporary directory");
    let trail = audit(&spool.path().join("spool"));
    let _stop = sender(&store, &consumers, &trail, &wake);
    until(&store, |rows| rows.iter().all(|row| row.3)).await;
    assert_eq!(2, refusing.received().len(), "both attempts reached it");
    let logs = capture.text();
    assert!(logs.contains("ITI-10"), "the notifier logged: {logs}");
    assert!(logs.contains("acknowledged AE"), "the failure is named");
    for (label, secret) in [
        ("the BSN", shared.as_str()),
        ("the hospital identifier", "H-63"),
        ("the family name", FAMILY_NAMES[5]),
    ] {
        assert!(!logs.contains(secret), "{label} reached the log");
    }
}
