// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-93 sent over PostgreSQL 18 (#104), behind `FERROPIX_E2E`:
//! - a change to a person writes one outbox row per matching subscription
//!   in its own transaction, and none for a subscription it does not match;
//! - each subscriber receives its messages in order;
//! - a subscriber that answers `5xx` is retried within the bounds, then its
//!   subscription is set to `error` with the text, and an enable resumes
//!   from the next message;
//! - no identifier or name reaches the log;
//! - a steward merge and its revert each queue one message per matching
//!   subscription.
//!
//! Every name, date and identifier is synthetic.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the messages are read as JSON values"
)]

use std::time::Duration;

use ferropix_server::config::section::domain::{DomainKind, DomainSeed};
use ferropix_server::config::section::outbox::Outbox;
use ferropix_server::feed::merging::{self, MergeRequest};
use ferropix_server::feed::{Context, Feeder};
use ferropix_server::outbox::{self, Backoff, Channels, HttpChannel, Wake};
use ferropix_server::store::Store;
use ferropix_server::store::record::{Channel, PersonId};
use ferropix_server::store::subscription::{self, FeedSettings, Updated};
use ferropix_testkit::containers::e2e_enabled;
use fhir_types::codec::Json as _;
use fhir_types::codec::Path;
use fhir_types::r4::patient::Patient;
use ihe_iti::pmir::server::subscription::RequestedSubscription;
use ihe_iti::server::format::Format;
use serde_json::Value;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::BSN_SYSTEM;
use super::leak::Capture;
use super::pixm::{Harness, SOURCE, synthetic_seeds};

/// The master domain: the harness's public base URL.
const MASTER: &str = "http://localhost:8080";

/// The first master identity.
const FIRST: &str = "4a0c2a5e-0000-4000-8000-000000000001";

/// The second master identity.
const SECOND: &str = "4a0c2a5e-0000-4000-8000-000000000002";

/// A synthetic BSN that passes the eleven-proof.
const BSN: &str = "111222333";

/// The family name the synthetic patients carry.
const FAMILY: &str = "Zandvoortje";

/// The seeds with the master domain, fed by [`SOURCE`].
fn with_master() -> Vec<DomainSeed> {
    let mut seeds = synthetic_seeds();
    seeds.push(DomainSeed {
        system: MASTER.to_owned(),
        name: String::from("Master Patient Identification Domain"),
        kind: DomainKind::Master,
        source_client_id: SOURCE.to_owned(),
        sensitive: false,
    });
    seeds
}

/// The harness's store announcing changes with `max_attempts`, woken by
/// `wake`.
fn feeding(harness: &Harness, max_attempts: u32, wake: &Wake) -> Store {
    harness.store.clone().with_feed(FeedSettings {
        registry: url::Url::parse(&format!("{MASTER}/fhir")).expect("a URL"),
        master: MASTER.to_owned(),
        max_attempts,
        wake: wake.clone(),
    })
}

/// The Source's context on the PMIR channel.
fn context() -> Context {
    Context {
        actor: SOURCE.to_owned(),
        channel: Channel::Pmir,
        request_id: None,
    }
}

/// The synthetic Patient `id`, carrying the BSN.
fn patient(id: &str, given: &str) -> Patient {
    let value = serde_json::json!({
        "resourceType": "Patient",
        "id": id,
        "identifier": [{"system": BSN_SYSTEM, "value": BSN}],
        "name": [{"family": FAMILY, "given": [given]}],
        "gender": "female",
        "birthDate": "1970-02-03"
    });
    let document: fhir_types::codec::Value = serde_json::from_value(value).expect("a document");
    Patient::from_json(
        document.as_object().expect("an object"),
        &mut Path::root("Patient"),
    )
    .expect("a Patient")
}

/// The person `id` names.
fn person(id: &str) -> PersonId {
    PersonId::from_uuid(Uuid::parse_str(id).expect("a UUID"))
}

/// Stores an active subscription with `criteria` delivering to `endpoint`.
async fn subscribe(store: &Store, criteria: &str, endpoint: &str) -> Uuid {
    let resource = request(criteria, endpoint, "requested");
    let requested = RequestedSubscription::read(Format::Json, resource.to_string().as_bytes())
        .expect("a PMIR Subscription");
    let created = subscription::create(store, &requested)
        .await
        .expect("stored");
    Uuid::parse_str(created.id.as_deref().expect("an id")).expect("a UUID")
}

/// The vendored PMIR Subscription request with `criteria`, `endpoint` and
/// `status`.
fn request(criteria: &str, endpoint: &str, status: &str) -> Value {
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/specs/ihe-pmir/package/example/Subscription-ex-subscription-request.json"
    );
    let mut value: Value =
        serde_json::from_str(&std::fs::read_to_string(file).expect("the example reads"))
            .expect("JSON");
    value["criteria"] = Value::String(criteria.to_owned());
    value["channel"]["endpoint"] = Value::String(endpoint.to_owned());
    value["status"] = Value::String(status.to_owned());
    value
}

/// The outbox's PMIR rows as `(subscription, created_at, delivered,
/// abandoned)`, oldest first.
async fn rows(store: &Store) -> Vec<(Uuid, String, bool, bool)> {
    sqlx::query_as(
        "SELECT subscription_id, created_at::text, delivered_at IS NOT NULL,
                abandoned_at IS NOT NULL
         FROM pix.outbox WHERE channel = 'pmir' ORDER BY created_at, id",
    )
    .fetch_all(store.pool())
    .await
    .expect("the outbox reads")
}

/// Starts the sender over `store` delivering ITI-93 with short waits.
fn sender(store: &Store, wake: &Wake) -> tokio::sync::watch::Sender<bool> {
    let channels = Channels {
        xcpd: None,
        pmir: Some(HttpChannel {
            client: reqwest::Client::new(),
            backoff: Backoff {
                initial: Duration::from_millis(20),
                max: Duration::from_millis(40),
            },
        }),
        hl7v2: None,
    };
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let retention = Outbox::default().resolve().expect("the default retention");
    let _sender = outbox::start(store.clone(), channels, retention, wake.clone(), stopped);
    stop
}

/// Waits until `done` holds of the PMIR rows.
async fn until(store: &Store, done: impl Fn(&[(Uuid, String, bool, bool)]) -> bool) {
    for _ in 0..400 {
        if done(&rows(store).await) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the outbox did not settle: {:?}", rows(store).await);
}

/// The history methods of each message `stub` received on `route`, in
/// order.
async fn methods(stub: &MockServer, route: &str) -> Vec<Vec<String>> {
    stub.received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.url.path() == route)
        .map(|request| {
            let message: Value = serde_json::from_slice(&request.body).expect("a JSON message");
            message["entry"][1]["resource"]["entry"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|entry| {
                    entry["request"]["method"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned()
                })
                .collect()
        })
        .collect()
}

#[tokio::test]
async fn each_matching_subscription_gets_its_messages_in_order_and_no_other() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(with_master()).await;
    let wake = Wake::default();
    let store = feeding(&harness, 3, &wake);
    let stub = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&stub)
        .await;
    let every = subscribe(&store, "Patient", &format!("{}/every", stub.uri())).await;
    let one = subscribe(
        &store,
        &format!("Patient?_id={FIRST}"),
        &format!("{}/one", stub.uri()),
    )
    .await;
    let _other = subscribe(
        &store,
        &format!("Patient?_id={SECOND}"),
        &format!("{}/other", stub.uri()),
    )
    .await;
    let feeder = Feeder::new(store.clone(), &harness.matching);
    feeder
        .create_master(MASTER, person(FIRST), &patient(FIRST, "Anna"), &context())
        .await
        .expect("created");
    let written = rows(&store).await;
    assert_eq!(2, written.len(), "two matching subscriptions: {written:?}");
    let mut named: Vec<Uuid> = written.iter().map(|row| row.0).collect();
    named.sort_unstable();
    let mut wanted = vec![every, one];
    wanted.sort_unstable();
    assert_eq!(wanted, named);
    assert!(
        written.iter().all(|row| row.1 == written[0].1),
        "written in the one transaction of the change"
    );
    feeder
        .update_master(MASTER, person(FIRST), &patient(FIRST, "Annie"), &context())
        .await
        .expect("updated");
    assert_eq!(4, rows(&store).await.len());

    let _stop = sender(&store, &wake);
    until(&store, |rows| rows.iter().all(|row| row.2)).await;
    for route in ["/every", "/one"] {
        assert_eq!(
            vec![vec![String::from("POST")], vec![String::from("PUT")]],
            methods(&stub, route).await,
            "{route}: the create, then the update"
        );
    }
    assert_eq!(Vec::<Vec<String>>::new(), methods(&stub, "/other").await);
}

#[tokio::test]
async fn a_refusing_subscriber_is_set_to_error_and_an_enable_resumes_from_the_next_message() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(with_master()).await;
    let wake = Wake::default();
    let store = feeding(&harness, 2, &wake);
    let stub = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/pmir"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&stub)
        .await;
    Mock::given(method("POST"))
        .and(path("/pmir"))
        .respond_with(ResponseTemplate::new(200))
        .with_priority(2)
        .mount(&stub)
        .await;
    let endpoint = format!("{}/pmir", stub.uri());
    let id = subscribe(&store, "Patient", &endpoint).await;
    let feeder = Feeder::new(store.clone(), &harness.matching);
    feeder
        .create_master(MASTER, person(FIRST), &patient(FIRST, "Anna"), &context())
        .await
        .expect("created");
    feeder
        .update_master(MASTER, person(FIRST), &patient(FIRST, "Annie"), &context())
        .await
        .expect("updated");

    let _stop = sender(&store, &wake);
    until(&store, |rows| rows.first().is_some_and(|row| row.3)).await;
    let read = subscription::read(&store, id)
        .await
        .expect("reads")
        .expect("there");
    assert_eq!(Some("error"), read.status.value.as_deref());
    assert!(
        read.error
            .as_ref()
            .and_then(|error| error.value.as_deref())
            .is_some_and(|error| error.contains("503")),
        "the error names the failure: {:?}",
        read.error
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        2,
        stub.received_requests().await.unwrap_or_default().len(),
        "two attempts within the bounds, and nothing sent while in error"
    );
    assert!(!rows(&store).await[1].2, "the next message waits");

    let enable = request("Patient", &endpoint, "requested");
    let requested = RequestedSubscription::read(Format::Json, enable.to_string().as_bytes())
        .expect("a PMIR Subscription");
    assert!(matches!(
        subscription::update(&store, id, &requested).await,
        Ok(Updated::Done(_))
    ));
    wake.now();
    until(&store, |rows| rows.get(1).is_some_and(|row| row.2)).await;
    assert_eq!(
        vec![vec![String::from("PUT")]],
        methods(&stub, "/pmir").await.split_off(2),
        "the enable resumes with the update"
    );
}

#[tokio::test]
async fn no_identifier_or_name_of_a_message_reaches_the_log() {
    if !e2e_enabled() {
        return;
    }
    let capture = Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());
    let harness = Harness::start(with_master()).await;
    let wake = Wake::default();
    let store = feeding(&harness, 3, &wake);
    let stub = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&stub)
        .await;
    subscribe(&store, "Patient", &format!("{}/pmir", stub.uri())).await;
    let feeder = Feeder::new(store.clone(), &harness.matching);
    feeder
        .create_master(MASTER, person(FIRST), &patient(FIRST, "Anna"), &context())
        .await
        .expect("created");
    let _stop = sender(&store, &wake);
    until(&store, |rows| rows.iter().all(|row| row.3)).await;
    let logs = capture.text();
    assert!(logs.contains("ITI-93"), "the sender logged: {logs}");
    for (label, secret) in [
        ("the BSN", BSN),
        ("the family name", FAMILY),
        ("the person id", FIRST),
    ] {
        assert!(!logs.contains(secret), "{label} reached the log");
    }
}

#[tokio::test]
async fn a_steward_merge_and_its_revert_each_queue_one_message_per_matching_subscription() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(with_master()).await;
    let wake = Wake::default();
    let store = feeding(&harness, 3, &wake);
    let feeder = Feeder::new(store.clone(), &harness.matching);
    for (id, given) in [(FIRST, "Anna"), (SECOND, "Bea")] {
        let mut fed = patient(id, given);
        fed.identifier.clear();
        feeder
            .create_master(MASTER, person(id), &fed, &context())
            .await
            .expect("created");
    }
    let every = subscribe(&store, "Patient", "https://subscriber.example.org/pmir").await;
    let _other = subscribe(
        &store,
        "Patient?_id=4a0c2a5e-0000-4000-8000-0000000000ff",
        "https://other.example.org/pmir",
    )
    .await;
    let version = |id: &'static str| {
        let store = store.clone();
        async move {
            let mut connection = store.pool().acquire().await.expect("a connection");
            merging::version(&mut connection, person(id))
                .await
                .expect("reads")
                .expect("a person")
        }
    };
    let request = MergeRequest {
        survivor: person(FIRST),
        survivor_version: version(FIRST).await,
        merged: person(SECOND),
        merged_version: version(SECOND).await,
        reason: String::from("one patient"),
    };
    let steward = Context {
        actor: String::from("synthetic-steward"),
        channel: Channel::Fhir,
        request_id: None,
    };
    let (transaction, change) = merging::merge(&store, &request, &steward)
        .await
        .expect("merged");
    transaction.commit().await.expect("committed");
    assert_eq!(
        vec![every],
        rows(&store)
            .await
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        "one message for the one matching subscription"
    );
    let (transaction, _reverted) = merging::revert(
        &store,
        change.merge,
        &version(FIRST).await,
        "two patients",
        &steward,
    )
    .await
    .expect("reverted");
    transaction.commit().await.expect("committed");
    assert_eq!(2, rows(&store).await.len(), "and one for the revert");
}
