// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The audit spool: the start or the request refused when it cannot take a
//! record, the drain that removes a delivered record and quarantines a
//! refused one, and the audit record as the only place an identifier
//! reaches the disk or the log.

#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the records are read as JSON values"
)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ferrohealth_health::registry::Registry;
use ferrohealth_server::health::Health;
use ferrohealth_telemetry::request_log::RequestLog;
use ferropix_server::audit::records::raw_request;
use ferropix_server::audit::reserve::UNAUDITED;
use ferropix_server::audit::{Audit, Headroom, OpenError};
use ferropix_server::auth::Smart;
use ferropix_server::config::section::audit::AuditSettings;
use ferropix_server::fhir::{self, Mounts};
use ferropix_server::http::router;
use http::{Method, StatusCode};
use ihe_atna::atna::spool::{QUARANTINE, SpoolError};
use ihe_atna::balp::{Exchange, Outcome};
use ihe_iti::pixm::audit::manager_query;
use ihe_iti::pixm::identifier::SourceIdentifier;
use secrecy::SecretString;
use serde_json::Value;
use wiremock::matchers;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::auth::{Captured, EVERY_SCOPE, Issuer, bearer, send, settings as auth_settings};
use crate::support::Secrets;

use super::{BASE, BSN_SYSTEM, bsn, call};

/// The `[audit]` settings of a spool under `directory`, delivering to
/// `url` when given.
fn audit_settings(directory: &Path, url: Option<&str>, max_events: usize) -> AuditSettings {
    AuditSettings {
        url: url.map(|url| url::Url::parse(url).expect("a URL")),
        spool_dir: directory.to_path_buf(),
        spool_max_bytes: 1 << 20,
        spool_max_events: max_events,
        spool_write_timeout: Duration::from_secs(2),
        timeout: Duration::from_secs(2),
        retry_max: Duration::from_millis(200),
    }
}

/// A record naming the synthetic BSN.
fn bsn_record() -> Exchange {
    let bsn = bsn();
    manager_query(
        &call(None).received(),
        raw_request(
            &url::Url::parse(&format!(
                "{BASE}/Patient/$ihe-pix?sourceIdentifier={BSN_SYSTEM}%7C{bsn}"
            ))
            .expect("a URL"),
            None,
        ),
        &SourceIdentifier::new(BSN_SYSTEM, SecretString::from(bsn)).expect("an identifier"),
        Outcome::Success,
    )
}

fn base_url() -> url::Url {
    url::Url::parse("https://pix.example.org").expect("a URL")
}

#[test]
fn a_spool_directory_open_to_other_users_or_unwritable_refuses_the_start() {
    let parent = tempfile::tempdir().expect("a temporary directory");
    let exposed = parent.path().join("exposed");
    std::fs::create_dir_all(&exposed).expect("the directory is made");
    std::fs::set_permissions(&exposed, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let error = Audit::open(&audit_settings(&exposed, None, 10), &base_url())
        .expect_err("a spool others may read is refused");
    assert!(
        matches!(error, OpenError::Spool(SpoolError::Exposed(_))),
        "{error:?}"
    );
    let unwritable = parent.path().join("unwritable");
    std::fs::create_dir_all(&unwritable).expect("the directory is made");
    std::fs::create_dir_all(unwritable.join(QUARANTINE)).expect("the quarantine is made");
    std::fs::set_permissions(
        unwritable.join(QUARANTINE),
        std::fs::Permissions::from_mode(0o700),
    )
    .expect("chmod");
    std::fs::set_permissions(&unwritable, std::fs::Permissions::from_mode(0o500)).expect("chmod");
    let refused = Audit::open(&audit_settings(&unwritable, None, 10), &base_url());
    std::fs::set_permissions(&unwritable, std::fs::Permissions::from_mode(0o700))
        .expect("chmod back");
    assert!(
        matches!(refused, Err(OpenError::Spool(_))),
        "a spool the server cannot write is refused: {refused:?}"
    );
}

#[tokio::test]
async fn a_full_spool_refuses_every_audited_request_with_503_and_readiness_names_it() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let settings = auth_settings(&issuer, &secrets);
    let backend = crate::auth::backend(&settings);
    let spool = tempfile::tempdir().expect("a temporary directory");
    let audit = Audit::open(
        &audit_settings(&spool.path().join("spool"), None, 2),
        &base_url(),
    )
    .expect("the spool opens");
    let smart = Arc::new(
        Smart::discover(settings.auth, &settings.domains)
            .await
            .expect("the authorization server is read"),
    );
    let health = Registry::default().and(Arc::new(Headroom(audit.clone())));
    let lifecycle = crate::support::serving();
    let app = router(
        &settings.server,
        Health::new(health.clone(), lifecycle.clone()),
        Some(fhir::routes(
            fhir::Address {
                base: settings.server.base_path.as_str(),
                public_base_url: &settings.public_base_url,
            },
            Mounts::all(),
            &smart,
            &health,
            &lifecycle,
            &audit,
            &backend,
        )),
        RequestLog::default(),
        None,
    );
    let token = bearer(&issuer.token("east-lab", EVERY_SCOPE));
    let admitted = send(&app, Method::POST, "/fhir/Patient/$ihe-pix", Some(&token)).await;
    assert_eq!(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        admitted.status,
        "room in the spool: the Manager reads the request, which posts no FHIR body"
    );
    let ready = send(&app, Method::GET, "/health/readiness", None).await;
    assert_eq!(StatusCode::OK, ready.status, "{}", ready.body);
    audit
        .record(vec![bsn_record(), bsn_record()])
        .await
        .expect("two records fill the spool");
    for (method, path) in [
        (Method::POST, "/fhir/Patient/$ihe-pix"),
        (Method::GET, "/fhir/Patient"),
        (Method::POST, "/operator/persons/p-1/reveal"),
    ] {
        let answer = send(&app, method, path, Some(&token)).await;
        assert_eq!(StatusCode::SERVICE_UNAVAILABLE, answer.status, "{path}");
        let outcome = answer.json();
        assert_eq!("transient", outcome["issue"][0]["code"], "{path}");
        assert_eq!(UNAUDITED, outcome["issue"][0]["diagnostics"], "{path}");
    }
    let dependencies = send(&app, Method::GET, "/operator/dependencies", Some(&token)).await;
    assert_eq!(
        StatusCode::OK,
        dependencies.status,
        "an unaudited read still answers"
    );
    let ready = send(&app, Method::GET, "/health/readiness", None).await;
    assert_eq!(StatusCode::SERVICE_UNAVAILABLE, ready.status);
    assert_eq!(
        serde_json::json!([{ "name": "audit_spool", "state": "down" }]),
        ready.json()["indicators"],
        "the open route names the indicator and never its detail"
    );
}

/// The stored records of the spool at `directory`, quarantined ones apart.
fn stored(directory: &Path) -> Vec<PathBuf> {
    files(directory)
        .into_iter()
        .filter(|path| path.parent() == Some(directory))
        .collect()
}

/// Every file under `directory`, recursively.
fn files(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).expect("the directory reads") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}

/// Waits until `condition` holds, for at most ten seconds.
async fn eventually(condition: impl Fn() -> bool) -> bool {
    for _ in 0..200 {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    condition()
}

#[tokio::test]
async fn a_delivered_record_is_removed_and_a_refused_one_quarantined() {
    let repository = MockServer::start().await;
    let spool = tempfile::tempdir().expect("a temporary directory");
    let directory = spool.path().join("spool");
    let audit = Audit::open(
        &audit_settings(&directory, Some(&format!("{}/fhir", repository.uri())), 10),
        &base_url(),
    )
    .expect("the spool and the feed open");
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/fhir/AuditEvent"))
        .respond_with(ResponseTemplate::new(201))
        .up_to_n_times(1)
        .mount(&repository)
        .await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/fhir/AuditEvent"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&repository)
        .await;
    let draining = audit.drain().expect("a repository is configured");
    audit
        .record(vec![bsn_record()])
        .await
        .expect("the record is stored");
    assert!(
        eventually(
            || stored(&directory).is_empty() && files(&directory.join(QUARANTINE)).is_empty()
        )
        .await,
        "the delivered record is removed"
    );
    // The forwarder removes a delivered record before it counts the delivery.
    assert!(
        eventually(|| {
            audit
                .upstream()
                .expect("the repository is named")
                .last
                .map(|last| last.state)
                == Some(ferrohealth_health::document::State::Up)
        })
        .await,
        "the repository reads up once the delivery is counted"
    );
    audit
        .record(vec![bsn_record()])
        .await
        .expect("the record is stored");
    assert!(
        eventually(
            || stored(&directory).is_empty() && files(&directory.join(QUARANTINE)).len() == 1
        )
        .await,
        "the refused record is quarantined"
    );
    draining.abort();
    let posted = repository.received_requests().await.unwrap_or_default();
    assert_eq!(2, posted.len());
    assert_eq!(
        Some("application/fhir+json"),
        posted[0]
            .headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
    );
}

#[tokio::test]
async fn the_audit_record_is_the_only_place_an_identifier_reaches_the_disk_or_the_log() {
    let bsn = bsn();
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let state = tempfile::tempdir().expect("a temporary directory");
    let directory = state.path().join("spool");
    let audit =
        Audit::open(&audit_settings(&directory, None, 10), &base_url()).expect("the spool opens");
    let exchange = bsn_record();
    assert!(
        !format!("{exchange:?}").contains(&bsn),
        "an exchange's Debug"
    );
    audit
        .record(vec![exchange])
        .await
        .expect("the record is stored");
    let holding: Vec<PathBuf> = files(state.path())
        .into_iter()
        .filter(|path| {
            std::fs::read(path).is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(&bsn))
        })
        .collect();
    assert_eq!(
        stored(&directory),
        holding,
        "only the spooled record holds the BSN"
    );
    let record: Value =
        serde_json::from_slice(&std::fs::read(&holding[0]).expect("the record reads"))
            .expect("JSON");
    assert_eq!("AuditEvent", record["resourceType"]);
    let mode = std::fs::metadata(&holding[0])
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(0o600, mode, "ITI TF-2 §3.20.4.1.1 spool: the owner alone");
    let log = String::from_utf8(captured.0.lock().expect("the buffer").clone()).expect("UTF-8");
    assert!(!log.contains(&bsn), "the log holds the BSN");
}
