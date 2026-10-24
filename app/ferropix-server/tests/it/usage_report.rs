// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The installation report to `FerroPULSE`: the reports the server builds,
//! each validated against the vendored envelope v2 schema and searched for
//! anything that names a patient, a domain, a host or a request path; the
//! window the request log fills; the sends to a mock collector; and the boot
//! line.
//!
//! The contract is `FerroPULSE` `docs/report-v2.md` at the commit
//! `docs/VERSIONS.md` pins; no other specification governs the report.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the vendored schema, its examples and the reports are read as JSON values"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ferropix_operator_api::health::{IndicatorState, State};
use ferropix_server::config::Profile;
use ferropix_server::config::section::usage_report::{Deployment, UsageReportSettings};
use ferropix_server::health::{Check, HealthIndicator, Registry};
use ferropix_server::usage_report::payload::Service;
use ferropix_server::usage_report::reporter::{
    self, Facts, Prepared, ReportError, Reporter, Sources, StartOutcome,
};
use ferropix_server::usage_report::window::Window;
use http::{Method, StatusCode};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::auth::{Captured, EVERY_SCOPE, Issuer, application, bearer, send, settings};
use crate::support::Secrets;

/// The vendored schema tree, `scripts/vendor/ferropulse-schemas.sh`.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ferropulse/schemas")
}

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).expect("the vendored file reads");
    serde_json::from_str(&text).expect("the vendored file is JSON")
}

/// The report envelope v2 validator.
fn validator() -> jsonschema::Validator {
    let schema = read_json(&fixtures().join("report-envelope.v2.json"));
    jsonschema::validator_for(&schema).expect("the schema compiles")
}

/// The examples of one validity, by file name.
fn examples(validity: &str) -> Vec<(String, Value)> {
    let dir = fixtures()
        .join("examples/report-envelope.v2")
        .join(validity);
    let mut out: Vec<(String, Value)> = std::fs::read_dir(&dir)
        .expect("the examples are vendored")
        .map(|entry| {
            let path = entry.expect("an entry").path();
            let name = path
                .file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned();
            (name, read_json(&path))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Fails unless `report` is valid against the vendored schema.
fn assert_valid(report: &Value) {
    let errors: Vec<String> = validator()
        .iter_errors(report)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}\n{report:#}");
}

/// Collects every string `value` holds, keys included.
fn strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(members) => {
            for (key, member) in members {
                out.push(key);
                strings(member, out);
            }
        }
        _ => {}
    }
}

/// Fails when `report` holds any of `forbidden`, or a string outside the
/// envelope's own members that is not a short lower-case token.
fn assert_names_nothing(report: &Value, forbidden: &[&str]) {
    let rendered = report.to_string();
    for needle in forbidden {
        assert!(
            !rendered.contains(needle),
            "the report holds {needle:?}: {rendered}"
        );
    }
    let object = report.as_object().expect("an object");
    let mut found = Vec::new();
    for (key, member) in object {
        if !["instance_id", "version", "git_sha"].contains(&key.as_str()) {
            strings(member, &mut found);
        }
    }
    for text in found {
        let token = !text.is_empty()
            && text.len() <= 32
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._+-<_".contains(&b));
        assert!(token, "{text:?} is not a short token: {rendered}");
    }
}

/// `report` with the members that depend on the host or the build replaced
/// by `*`, after checking it against the schema.
fn stable(mut report: Value) -> Value {
    assert_valid(&report);
    let object = report.as_object_mut().expect("an object");
    let id = object["instance_id"].as_str().expect("a string");
    assert_eq!(id.len(), 36, "hyphenated: {id}");
    for member in [
        "instance_id",
        "git_sha",
        "uptime_s",
        "cpu_bucket",
        "memory_bucket",
    ] {
        object.insert(member.to_owned(), json!("*"));
    }
    report
}

/// An indicator that answers the same state every time.
#[derive(Debug)]
struct Fixed(&'static str, State);

impl HealthIndicator for Fixed {
    fn name(&self) -> &'static str {
        self.0
    }

    fn check(&self) -> Check<'_> {
        let state = self.1;
        Box::pin(async move {
            match state {
                State::Up => IndicatorState::up(),
                State::Down => IndicatorState::down("the synthetic indicator is down"),
            }
        })
    }
}

/// A store that answers and an audit spool without headroom.
fn health() -> Registry {
    Registry::default()
        .and(Arc::new(Fixed("database", State::Up)))
        .and(Arc::new(Fixed("audit_spool", State::Down)))
}

/// The sources of a production server with PIXm, PDQm and the operator API
/// on, three domains in the store, and `window`.
fn sources(window: Arc<Window>) -> Sources {
    Sources {
        facts: Facts::new(
            Profile::Production,
            vec![
                Service::Pixm,
                Service::Operator,
                Service::Pdqm,
                Service::Pixm,
            ],
            3,
        ),
        health: health(),
        window,
    }
}

/// A report configuration over `state_dir` posting to `endpoint`.
fn config(state_dir: &Path, endpoint: &str) -> UsageReportSettings {
    UsageReportSettings {
        enabled: true,
        endpoint: url::Url::parse(endpoint).expect("a URL"),
        state_dir: Some(state_dir.to_path_buf()),
        deployment: Deployment::Compose,
    }
}

fn prepared(config: &UsageReportSettings) -> Prepared {
    reporter::prepare(config, None).expect("the report runs")
}

fn reporter_over(config: &UsageReportSettings, sources: Sources) -> Reporter {
    Reporter::new(config, prepared(config), sources, Instant::now()).expect("the client builds")
}

const COLLECTOR: &str = "https://report.ferropulse.eu/v1/report";

#[test]
fn the_validator_admits_every_valid_example_and_refuses_every_invalid_one() {
    let validator = validator();
    let valid = examples("valid");
    let invalid = examples("invalid");
    assert!(
        valid.iter().any(|(name, _)| name == "ferropix-start.json"),
        "the pin carries the contract that takes ferropix"
    );
    assert!(!invalid.is_empty(), "the examples are vendored");
    for (name, example) in valid {
        assert!(validator.is_valid(&example), "{name} should be valid");
    }
    for (name, example) in invalid {
        assert!(!validator.is_valid(&example), "{name} should be refused");
    }
}

#[tokio::test]
async fn the_start_report_is_exactly_the_v2_envelope() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config(dir.path(), COLLECTOR);
    let reporter = reporter_over(&config, sources(Arc::new(Window::new(""))));
    let report = serde_json::to_value(reporter.start_report().await).expect("serializes");
    assert_eq!(
        stable(report),
        json!({
            "schema": 2,
            "event": "start",
            "first_start": true,
            "product": "ferropix",
            "instance_id": "*",
            "version": env!("CARGO_PKG_VERSION"),
            "git_sha": "*",
            "licence": "non-commercial",
            "deployment": "compose",
            "uptime_s": "*",
            "cpu_bucket": "*",
            "memory_bucket": "*",
            "status": {
                "profile": "production",
                "services": ["operator", "pdqm", "pixm"],
                "domains": "3-5",
                "db_ok": true,
                "audit_spool_ok": false
            }
        })
    );
}

#[tokio::test]
async fn an_idle_daily_report_carries_an_empty_window() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config(dir.path(), COLLECTOR);
    let bare = Sources {
        facts: Facts::new(Profile::Development, Vec::new(), 0),
        health: Registry::default(),
        window: Arc::new(Window::new("")),
    };
    let window = Arc::clone(&bare.window);
    let reporter = reporter_over(&config, bare);
    let report = stable(
        serde_json::to_value(reporter.daily_report(&window.take()).await).expect("serializes"),
    );
    assert_eq!(
        report["status"],
        json!({ "profile": "development", "services": [], "domains": "0" })
    );
    assert_eq!(
        report["metrics"],
        json!({
            "window_24h": {
                "requests_bucket": "0",
                "groups": {},
                "counters": { "responses_4xx": 0 }
            }
        })
    );
    assert!(report.get("first_start").is_none(), "start only");
}

#[tokio::test]
async fn every_report_counts_by_route_group_and_names_no_patient_domain_host_or_path() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let settings = settings(&issuer, &secrets);
    let domains: Vec<String> = settings
        .domains
        .iter()
        .flat_map(|domain| [domain.system.clone(), domain.name.clone()])
        .collect();
    assert!(!domains.is_empty(), "the server seeds a domain");
    let (app, _, _spool) = application(settings).await;
    let window = Arc::new(Window::new(""));
    let app = app.layer(axum::Extension(Arc::clone(&window)));
    let bsn = ferropix_testkit::synthetic::bsn(1)
        .expect("the synthetic range holds a BSN")
        .digits();
    let token = bearer(&issuer.token("east-lab", EVERY_SCOPE));
    let pix = format!(
        "/fhir/Patient/$ihe-pix?sourceIdentifier=http%3A%2F%2Ffhir.nl%2Ffhir%2FNamingSystem%2Fbsn%7C{bsn}"
    );
    send(&app, Method::GET, &pix, Some(&token)).await;
    send(&app, Method::GET, &pix, None).await;
    let read = format!("/fhir/Patient/{bsn}");
    send(&app, Method::GET, &read, Some(&token)).await;
    send(&app, Method::GET, "/fhir/metadata", None).await;
    send(&app, Method::GET, "/operator/dependencies", Some(&token)).await;
    send(&app, Method::GET, &format!("/{bsn}/unrouted"), None).await;
    send(&app, Method::GET, "/health/readiness", None).await;

    let dir = tempfile::tempdir().expect("tempdir");
    let config = config(dir.path(), COLLECTOR);
    let reporter = reporter_over(&config, sources(Arc::clone(&window)));
    let counts = window.take();
    let start = serde_json::to_value(reporter.start_report().await).expect("serializes");
    let daily = serde_json::to_value(reporter.daily_report(&counts).await).expect("serializes");
    let host = std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    let mut forbidden: Vec<&str> = vec![
        bsn.as_str(),
        "Patient",
        "ihe-pix",
        "/fhir",
        "localhost",
        "east-lab",
        "report.ferropulse.eu",
    ];
    forbidden.extend(domains.iter().map(String::as_str));
    forbidden.extend(host.as_deref());
    for report in [&start, &daily] {
        assert_valid(report);
        assert_names_nothing(report, &forbidden);
    }

    let window = &daily["metrics"]["window_24h"];
    assert_eq!(window["requests_bucket"], json!("1-100"));
    let groups = window["groups"].as_object().expect("an object");
    assert_eq!(
        groups.keys().map(String::as_str).collect::<Vec<_>>(),
        ["metadata", "operator", "other", "pdq_query", "pix_query"],
        "the probes are not counted and a group without requests is left out"
    );
    let total = |group: &str| -> u64 {
        groups[group]["histogram"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|count| count.as_u64().expect("a count"))
            .sum()
    };
    assert_eq!(total("pix_query"), 2);
    assert_eq!(total("pdq_query"), 1);
    assert_eq!(total("metadata"), 1);
    assert_eq!(total("operator"), 1);
    assert_eq!(total("other"), 1);
    let refusals = window["counters"]["responses_4xx"]
        .as_u64()
        .expect("a count");
    assert!(
        refusals >= 2,
        "the anonymous query and the unrouted path: {refusals}"
    );
}

#[tokio::test]
async fn the_start_report_reaches_the_collector_once_per_ten_minutes() {
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/report"))
        .and(header("content-type", "application/json"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&collector)
        .await;
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config(dir.path(), &format!("{}/v1/report", collector.uri()));
    let reporter = reporter_over(&config, sources(Arc::new(Window::new(""))));
    let first = reporter.send_start().await.expect("sent");
    assert_eq!(first, StartOutcome::Sent(StatusCode::NO_CONTENT));
    let again = reporter.send_start().await.expect("decided");
    assert_eq!(again, StartOutcome::RateLimited);
    let received = collector.received_requests().await.expect("recorded");
    assert_eq!(1, received.len(), "one start report");
    let request = received.first().expect("one start report");
    let user_agent = request
        .headers
        .get("user-agent")
        .and_then(|value| value.to_str().ok())
        .expect("a user agent");
    assert_eq!(
        user_agent,
        format!("ferropix/{}", env!("CARGO_PKG_VERSION"))
    );
    let body: Value = serde_json::from_slice(&request.body).expect("a JSON body");
    assert_eq!(body["event"], json!("start"));
    assert_valid(&body);
}

#[tokio::test]
async fn a_daily_report_the_collector_answers_ends_its_window_even_when_refused() {
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/report"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&collector)
        .await;
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config(dir.path(), &format!("{}/v1/report", collector.uri()));
    let window = Arc::new(Window::new(""));
    window.record(
        &Method::GET,
        Some("/fhir/metadata"),
        StatusCode::OK,
        Duration::from_millis(2),
    );
    let reporter = reporter_over(&config, sources(Arc::clone(&window)));
    let status = reporter.send_daily().await.expect("answered");
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(window.take().requests(), 0);
    let received = collector.received_requests().await.expect("recorded");
    let body: Value =
        serde_json::from_slice(&received.first().expect("one report").body).expect("JSON");
    assert_eq!(body["event"], json!("daily"));
    assert!(body["metrics"]["window_24h"]["groups"]["metadata"].is_object());
    assert_valid(&body);
}

#[tokio::test]
async fn a_daily_report_that_reaches_no_collector_keeps_its_window() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = closed.local_addr().expect("an address").port();
    drop(closed);
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config(dir.path(), &format!("http://127.0.0.1:{port}/v1/report"));
    let window = Arc::new(Window::new(""));
    window.record(
        &Method::GET,
        Some("/fhir/metadata"),
        StatusCode::OK,
        Duration::from_millis(2),
    );
    let reporter = reporter_over(&config, sources(Arc::clone(&window)));
    let error = reporter.send_daily().await.expect_err("nothing listens");
    assert!(matches!(error, ReportError::Request(_)), "{error:?}");
    assert_eq!(window.take().requests(), 1, "the retry carries the window");
}

/// Runs `body` on a current-thread runtime with every log line kept in
/// `logs`, so the task the report spawns logs into it too.
fn with_logs<F: Future<Output = ()>>(logs: &Captured, body: impl FnOnce() -> F) {
    let capture = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_writer(logs.clone())
        .finish();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    tracing::subscriber::with_default(capture, || runtime.block_on(body()));
}

fn text(logs: &Captured) -> String {
    String::from_utf8(logs.0.lock().expect("the buffer").clone()).expect("UTF-8")
}

#[test]
fn start_logs_the_boot_line_and_sends_the_start_report() {
    let logs = Captured::default();
    with_logs(&logs, || async {
        let collector = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/report"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&collector)
            .await;
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config(dir.path(), &format!("{}/v1/report", collector.uri()));
        let handle = reporter::start(
            &config,
            sources(Arc::new(Window::new(""))),
            Instant::now(),
            None,
        )
        .await
        .expect("the report runs");
        let deadline = Instant::now() + Duration::from_secs(10);
        while collector
            .received_requests()
            .await
            .is_none_or(|received| received.is_empty())
        {
            assert!(Instant::now() < deadline, "the start report did not arrive");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        handle.abort();
    });
    let text = text(&logs);
    assert!(text.contains("usage report ON: "), "{text}");
    assert!(
        text.contains("Off: FERROPIX__USAGE_REPORT__ENABLED=false"),
        "{text}"
    );
}

#[test]
fn a_ci_environment_keeps_the_report_off_and_writes_nothing() {
    let logs = Captured::default();
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    with_logs(&logs, || async {
        let config = config(&state_dir, COLLECTOR);
        let handle = reporter::start(
            &config,
            sources(Arc::new(Window::new(""))),
            Instant::now(),
            Some("GITHUB_ACTIONS"),
        )
        .await;
        assert!(handle.is_none(), "an explicit on does not override CI");
    });
    let text = text(&logs);
    assert!(
        text.contains("usage report OFF: CI detected (GITHUB_ACTIONS); nothing is sent"),
        "{text}"
    );
    assert!(
        !state_dir.exists(),
        "nothing is written while the report is off"
    );
}

#[test]
fn a_switched_off_report_says_so_and_sends_nothing() {
    let logs = Captured::default();
    let dir = tempfile::tempdir().expect("tempdir");
    with_logs(&logs, || async {
        let config = UsageReportSettings {
            enabled: false,
            ..config(dir.path(), COLLECTOR)
        };
        assert!(
            reporter::start(
                &config,
                sources(Arc::new(Window::new(""))),
                Instant::now(),
                None
            )
            .await
            .is_none()
        );
    });
    let text = text(&logs);
    assert!(
        text.contains("usage report OFF: switched off (FERROPIX__USAGE_REPORT__ENABLED=false)"),
        "{text}"
    );
}

#[test]
fn a_server_without_a_state_directory_reports_nothing() {
    let logs = Captured::default();
    with_logs(&logs, || async {
        let config = UsageReportSettings {
            state_dir: None,
            ..config(Path::new("/nonexistent"), COLLECTOR)
        };
        assert!(
            reporter::start(
                &config,
                sources(Arc::new(Window::new(""))),
                Instant::now(),
                None
            )
            .await
            .is_none()
        );
    });
    let text = text(&logs);
    assert!(
        text.contains("usage report OFF: no state directory (FERROPIX__USAGE_REPORT__STATE_DIR"),
        "{text}"
    );
}
