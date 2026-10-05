// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ATX: FHIR Feed: each stored record is posted to `[base]/AuditEvent`
//! as FHIR JSON (the `RESTful` ATNA supplement, ITI TF-2 §3.20.4.2), stays
//! spooled while the repository cannot take it (§3.20.4.1.1, which the
//! forwarder applies to the feed too), and a record the repository refuses is
//! quarantined so the drain goes on (§3.20.4.3.3).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ihe_iti::atna::feed::{FeedAddressError, FeedRepository};
use ihe_iti::atna::forwarder::Forwarder;
use ihe_iti::atna::spool::{Bounds, Content, QUARANTINE, Spool};
use ihe_iti::balp::{
    DESTINATION_ROLE, Direction, Entity, EventKind, Exchange, Outcome, Peer, REST, SEARCH,
    SOURCE_ROLE,
};
use secrecy::{ExposeSecret as _, SecretSlice, SecretString};
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::observer;
use crate::atna::until;
use crate::timing;

const BOUNDS: Bounds = Bounds {
    max_messages: 16,
    max_bytes: 64 << 20,
    write_timeout: Duration::from_secs(10),
};

/// The timeout of each request to the repository.
const REQUEST: Duration = Duration::from_secs(2);

const RETRY_MAX: Duration = Duration::from_millis(400);

/// The longest one failed delivery may hold the forwarder: its request and
/// the backoff after it.
const FAILED_ATTEMPT: Duration = REQUEST.saturating_add(RETRY_MAX);

/// A search on the BALP Patient Query pattern.
const KIND: EventKind = EventKind {
    profile: "https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.PatientQuery",
    event_type: REST,
    subtypes: &[SEARCH],
    action: "E",
    client: SOURCE_ROLE,
    server: DESTINATION_ROLE,
};

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("an HTTP client")
}

/// The repository at `server`'s `/arr/` base, over clear text for the test.
fn repository(server: &MockServer) -> FeedRepository {
    let base = Url::parse(&format!("{}/arr/", server.uri())).expect("a base");
    FeedRepository::cleartext_for_development(base, http(), REQUEST).expect("a repository")
}

/// One synthetic ITI-83 record naming the patient `value`.
fn record(value: &str) -> SecretSlice<u8> {
    Exchange {
        kind: KIND,
        recorded: jiff::Timestamp::UNIX_EPOCH,
        outcome: Outcome::Success,
        direction: Direction::Sent {
            server: Peer::server(&Url::parse("https://pix.example.org/fhir/").expect("a URL")),
        },
        on_behalf: ihe_iti::user::OnBehalfOf::System,
        entities: vec![Entity::Patient {
            system: "urn:oid:2.999.1".to_owned(),
            value: SecretString::from(value),
        }],
    }
    .audit_event(&observer())
    .expect("a record")
    .into_bytes()
}

async fn answering(server: &MockServer, status: u16) {
    Mock::given(method("POST"))
        .and(path("/arr/AuditEvent"))
        .and(header("content-type", "application/fhir+json"))
        .respond_with(ResponseTemplate::new(status))
        .mount(server)
        .await;
}

/// A repository that answers `503` until `taking` is set and `201` from
/// then on, so no request finds the repository without an answer between
/// the two.
async fn answering_once_taking(server: &MockServer, taking: Arc<AtomicBool>) {
    Mock::given(method("POST"))
        .and(path("/arr/AuditEvent"))
        .and(header("content-type", "application/fhir+json"))
        .respond_with(move |_: &wiremock::Request| {
            ResponseTemplate::new(if taking.load(Ordering::SeqCst) {
                201
            } else {
                503
            })
        })
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_stored_record_is_created_at_the_repository_as_fhir_json() {
    let server = MockServer::start().await;
    answering(&server, 201).await;
    let forwarder = Forwarder::fhir_feed(Spool::in_memory(BOUNDS), repository(&server), RETRY_MAX);
    let sent = record("Qz7-feed-1");
    forwarder
        .submit(SecretSlice::from(sent.expose_secret().to_vec()))
        .await
        .expect("stored");
    let running = tokio::spawn(Arc::clone(&forwarder).run());
    let status = until(&forwarder, timing::deadline(REQUEST), |status| {
        status.delivered == 1
    })
    .await;
    running.abort();
    assert_eq!(1, status.delivered);
    assert_eq!(0, status.depth.messages);
    let received = server.received_requests().await.expect("requests");
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].body, sent.expose_secret());
}

#[tokio::test]
async fn a_repository_that_cannot_take_records_leaves_them_spooled_until_it_can() {
    let server = MockServer::start().await;
    let taking = Arc::new(AtomicBool::new(false));
    answering_once_taking(&server, Arc::clone(&taking)).await;
    let forwarder = Forwarder::fhir_feed(Spool::in_memory(BOUNDS), repository(&server), RETRY_MAX);
    let running = tokio::spawn(Arc::clone(&forwarder).run());
    for value in ["Qz7-feed-2", "Qz7-feed-3"] {
        forwarder.submit(record(value)).await.expect("stored");
    }
    let down = until(
        &forwarder,
        timing::deadline(FAILED_ATTEMPT.saturating_mul(2)),
        |status| status.retries >= 2,
    )
    .await;
    assert!(!down.reachable);
    assert_eq!(2, down.depth.waiting(), "nothing is dropped");
    assert_eq!(0, down.delivered);
    taking.store(true, Ordering::SeqCst);
    // The forwarder may be in a failing attempt: it ends that and its
    // backoff, then posts both records, each within the request timeout.
    let recovery = FAILED_ATTEMPT.saturating_add(REQUEST.saturating_mul(2));
    let up = until(&forwarder, timing::deadline(recovery), |status| {
        status.delivered == 2
    })
    .await;
    running.abort();
    assert_eq!(2, up.delivered, "delivered once the repository takes them");
    assert_eq!(0, up.depth.messages);
}

#[tokio::test]
async fn a_refused_record_is_quarantined_and_the_drain_goes_on() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    answering(&server, 201).await;
    let directory = tempfile::tempdir().expect("a directory");
    let spool_dir = directory.path().join("feed-spool");
    let spool = Spool::open_for(&spool_dir, BOUNDS, Content::AuditEvents).expect("a spool");
    let forwarder = Forwarder::fhir_feed(spool, repository(&server), RETRY_MAX);
    for value in ["Qz7-feed-4", "Qz7-feed-5"] {
        forwarder.submit(record(value)).await.expect("stored");
    }
    let running = tokio::spawn(Arc::clone(&forwarder).run());
    let status = until(
        &forwarder,
        timing::deadline(REQUEST.saturating_mul(2)),
        |status| status.delivered == 1 && status.depth.quarantined == 1,
    )
    .await;
    running.abort();
    assert_eq!(1, status.delivered, "the next record is delivered");
    assert_eq!(1, status.depth.quarantined, "the refused one is held");
    assert_eq!(
        1,
        std::fs::read_dir(spool_dir.join(QUARANTINE))
            .expect("the quarantine")
            .count()
    );
}

#[tokio::test]
async fn a_spooled_file_that_is_no_audit_event_is_quarantined() {
    let directory = tempfile::tempdir().expect("a directory");
    let spool_dir = directory.path().join("feed-spool");
    let spool = Spool::open_for(&spool_dir, BOUNDS, Content::AuditEvents).expect("a spool");
    spool
        .push(SecretSlice::from(b"12 not a record".to_vec()))
        .await
        .expect("stored");
    spool.push(record("Qz7-feed-6")).await.expect("stored");
    let oldest = spool.oldest().await.expect("read").expect("a record");
    assert_eq!(1, oldest.sequence, "the frame is passed over");
    assert_eq!(1, spool.depth().quarantined);
}

#[test]
fn a_repository_base_in_clear_text_or_with_a_query_is_refused() {
    let refused = |base: &str| {
        FeedRepository::new(
            Url::parse(base).expect("a URL"),
            http(),
            Duration::from_secs(1),
        )
        .err()
    };
    assert_eq!(
        refused("http://arr.example.org/fhir/"),
        Some(FeedAddressError::Cleartext)
    );
    assert_eq!(
        refused("https://arr.example.org/fhir?x=1"),
        Some(FeedAddressError::Base)
    );
    assert_eq!(
        refused("tls://arr.example.org:6514"),
        Some(FeedAddressError::Base)
    );
    let accepted = FeedRepository::new(
        Url::parse("https://arr.example.org/fhir").expect("a URL"),
        http(),
        Duration::from_secs(1),
    )
    .expect("an https base");
    assert_eq!(
        accepted.endpoint().as_str(),
        "https://arr.example.org/fhir/AuditEvent"
    );
}

/// The bytes of a vendored file, by its path under `docs/specs`.
fn vendored_bytes(file: &str) -> Vec<u8> {
    let path: std::path::PathBuf = [env!("CARGO_MANIFEST_DIR"), "../../docs/specs", file]
        .iter()
        .collect();
    std::fs::read(&path).unwrap_or_else(|error| panic!("the vendored {} ({error})", path.display()))
}

#[test]
fn the_store_and_forward_the_spool_keeps_is_the_vendored_iti_20_text() {
    let page = String::from_utf8(vendored_bytes("ihe-atna/Volume2/ITI-20.html")).expect("UTF-8");
    assert!(page.contains("3.20.4.1.1 Trigger Events"));
    assert!(
        page.contains("the actor shall store the audit record locally and send it when it is able"),
        "ITI TF-2 §3.20.4.1.1, which the spool keeps"
    );
    let supplement = vendored_bytes("ihe-atna/IHE_ITI_Suppl_RESTful-ATNA.pdf");
    assert!(
        supplement.starts_with(b"%PDF"),
        "the supplement as published"
    );
    assert!(
        supplement
            .windows(31)
            .any(|window| window == b"IHE ITI RESTful ATNA Supplement"),
        "the RESTful ATNA supplement the FHIR Feed follows"
    );
}

#[test]
#[expect(
    clippy::disallowed_types,
    reason = "the test seam: the vendored capability statements are read as JSON values"
)]
fn the_feed_is_the_create_both_balp_capability_statements_name() {
    for file in [
        "ihe-balp/package/CapabilityStatement-IHE.BALP.AuditCreator.json",
        "ihe-balp/package/CapabilityStatement-IHE.BALP.ATNA.AuditRecordRepository.json",
    ] {
        let statement: serde_json::Value =
            serde_json::from_slice(&vendored_bytes(file)).expect("JSON");
        let creates = statement["rest"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|rest| rest["resource"].as_array().into_iter().flatten())
            .filter(|resource| resource["type"] == "AuditEvent")
            .flat_map(|resource| resource["interaction"].as_array().into_iter().flatten())
            .any(|interaction| interaction["code"] == "create");
        assert!(creates, "{file} names create on AuditEvent");
    }
}
