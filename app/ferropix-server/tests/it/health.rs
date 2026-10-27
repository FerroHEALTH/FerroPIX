// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The health family and the request log: readiness over the indicator
//! registry, `healthcheck` on a server that is not ready, the dependency
//! report behind the steward scope, and a request log that records the route
//! template and never the identifier a `GET` carries in its query (#52).
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the readiness and dependency documents are read as JSON values"
)]

use std::process::Command;
use std::sync::Arc;

use ferrohealth_health::document::State;
use ferrohealth_health::lifecycle::Lifecycle;
use ferrohealth_health::registry::{Check, HealthIndicator, Outcome, Registry};
use ferrohealth_server::health::Health;
use ferrohealth_server::section::ServerSection;
use ferrohealth_telemetry::request_log::RequestLog;
use ferropix_server::http::router;
use http::{Method, StatusCode};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::auth::{
    Captured, EVERY_SCOPE, Issuer, application, bearer, send, settings as auth_settings,
};
use crate::support::Secrets;

/// An indicator that always answers `state`.
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
                State::Up => Outcome::up(),
                State::Degraded => Outcome::degraded("the stub is slow"),
                State::Down => Outcome::down("the stub is down"),
            }
        })
    }
}

/// The settings of a server on a free loopback port at the root.
fn server() -> ServerSection {
    ServerSection {
        listen: "127.0.0.1:0".parse().expect("a socket address"),
        body_limit_bytes: 1024,
        request_timeout_ms: 5000,
        shutdown_timeout_ms: 300,
        ..ServerSection::default()
    }
}

/// Returns the health of a serving process over the indicators named, each
/// in its state.
fn registry(indicators: &[(&'static str, State)]) -> Health {
    let registry = indicators
        .iter()
        .fold(Registry::default(), |registry, (name, state)| {
            registry.and(Arc::new(Fixed(name, *state)))
        });
    let lifecycle = Lifecycle::default();
    lifecycle.booted();
    Health::new(registry, lifecycle)
}

#[tokio::test]
async fn readiness_answers_503_naming_each_down_indicator_and_never_its_detail() {
    let server = server();
    for (indicators, expected, down) in [
        (vec![], StatusCode::OK, vec![]),
        (
            vec![("database", State::Up), ("audit_spool", State::Up)],
            StatusCode::OK,
            vec![],
        ),
        (
            vec![("database", State::Down), ("audit_spool", State::Up)],
            StatusCode::SERVICE_UNAVAILABLE,
            vec!["database"],
        ),
        (
            vec![("database", State::Down), ("audit_spool", State::Down)],
            StatusCode::SERVICE_UNAVAILABLE,
            vec!["audit_spool", "database"],
        ),
    ] {
        let app = router(
            &server,
            registry(&indicators),
            None,
            RequestLog::default(),
            None,
        );
        let answer = send(&app, Method::GET, "/health/readiness", None).await;
        assert_eq!(expected, answer.status, "{indicators:?}");
        let document = answer.json();
        let listed = document["indicators"]
            .as_array()
            .expect("the indicators are a list");
        let named: Vec<&str> = listed
            .iter()
            .filter(|state| state["state"] == "down")
            .filter_map(|state| state["name"].as_str())
            .collect();
        assert_eq!(down, named, "{indicators:?}");
        assert!(
            listed.iter().all(|state| state.get("detail").is_none()),
            "the open route names no detail: {document}"
        );
        let aggregate = if down.is_empty() { "up" } else { "down" };
        assert_eq!(aggregate, document["state"], "{indicators:?}");
        assert_eq!("serving", document["phase"], "{indicators:?}");
        let summary = send(&app, Method::GET, "/health", None).await;
        assert_eq!(
            expected, summary.status,
            "the summary answers readiness's status"
        );
        assert_eq!(
            serde_json::json!({ "state": aggregate }),
            summary.json(),
            "the summary is the state alone"
        );
        let live = send(&app, Method::GET, "/health/liveness", None).await;
        assert_eq!(StatusCode::OK, live.status, "liveness asks no indicator");
    }
}

#[tokio::test]
async fn readiness_answers_503_while_booting_and_from_the_moment_the_drain_starts() {
    let lifecycle = Lifecycle::default();
    let health = Health::new(registry_of_up(), lifecycle.clone());
    let app = router(&server(), health, None, RequestLog::default(), None);
    for (phase, expected) in [
        ("booting", StatusCode::SERVICE_UNAVAILABLE),
        ("serving", StatusCode::OK),
        ("draining", StatusCode::SERVICE_UNAVAILABLE),
    ] {
        match phase {
            "serving" => lifecycle.booted(),
            "draining" => lifecycle.drain(),
            _ => {}
        }
        let answer = send(&app, Method::GET, "/health/readiness", None).await;
        assert_eq!(expected, answer.status, "{phase}");
        assert_eq!(phase, answer.json()["phase"], "{phase}");
        let live = send(&app, Method::GET, "/health/liveness", None).await;
        assert_eq!(
            StatusCode::OK,
            live.status,
            "{phase}: liveness asks nothing"
        );
    }
}

/// A registry whose one indicator is up.
fn registry_of_up() -> Registry {
    Registry::default().and(Arc::new(Fixed("database", State::Up)))
}

#[tokio::test(flavor = "multi_thread")]
async fn healthcheck_exits_one_on_a_server_whose_readiness_answers_503() {
    let secrets = Secrets::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let port = listener.local_addr().expect("an address").port();
    let app = router(
        &server(),
        registry(&[("database", State::Down)]),
        None,
        RequestLog::default(),
        None,
    );
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        ferrohealth_server::serve::run(listener, app, &server(), Lifecycle::default(), async move {
            stopped.await.unwrap_or(());
        })
        .await
    });
    let quickstart = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("quickstart.toml");
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_ferropix"))
            .args([
                "healthcheck",
                "--config",
                quickstart.to_str().expect("a UTF-8 path"),
            ])
            .env_clear()
            .envs(secrets.environment())
            .env("FERROPIX__SERVER__LISTEN", format!("127.0.0.1:{port}"))
            .output()
            .expect("the binary runs")
    })
    .await
    .expect("the job ends");
    stop.send(()).expect("the server is running");
    task.await
        .expect("the task ends")
        .expect("the server stops");
    assert_eq!(Some(1), output.status.code());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("503"), "{stdout}");
}

#[tokio::test]
async fn the_dependency_report_is_the_stewards_and_carries_the_readiness_report() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(auth_settings(&issuer, &secrets)).await;
    let anonymous = send(&app, Method::GET, "/operator/dependencies", None).await;
    assert_eq!(StatusCode::UNAUTHORIZED, anonymous.status);
    let consumer = bearer(&issuer.token("east-lab", "system/Patient.rs"));
    let refused = send(&app, Method::GET, "/operator/dependencies", Some(&consumer)).await;
    assert_eq!(StatusCode::FORBIDDEN, refused.status);
    let steward = bearer(&issuer.token("console", "__ferropix.steward"));
    let answer = send(&app, Method::GET, "/operator/dependencies", Some(&steward)).await;
    assert_eq!(StatusCode::OK, answer.status);
    let document = answer.json();
    assert_eq!("up", document["readiness"]["state"]);
    assert!(document["upstreams"].is_array());
}

#[tokio::test]
async fn a_bsn_sent_by_get_to_ihe_pix_appears_in_no_log_line_or_span() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(auth_settings(&issuer, &secrets)).await;
    let bsn = ferropix_testkit::synthetic::bsn(0)
        .expect("the synthetic range holds a BSN")
        .digits();
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_current_span(true)
        .with_span_list(true)
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let token = bearer(&issuer.token("east-lab", EVERY_SCOPE));
    let uri = format!(
        "/fhir/Patient/$ihe-pix?sourceIdentifier=http%3A%2F%2Ffhir.nl%2Ffhir%2FNamingSystem%2Fbsn%7C{bsn}&targetSystem={}",
        crate::auth::DOMAIN.replace(':', "%3A")
    );
    let answer = send(&app, Method::GET, &uri, Some(&token)).await;
    assert_eq!(
        StatusCode::SERVICE_UNAVAILABLE,
        answer.status,
        "admitted, and the store at an unresolvable host cannot be read"
    );
    let anonymous = send(&app, Method::GET, &uri, None).await;
    assert_eq!(StatusCode::UNAUTHORIZED, anonymous.status);
    let unmatched = format!("/fhir/Patient/{bsn}/_history?x={bsn}");
    send(&app, Method::GET, &unmatched, Some(&token)).await;
    let log = String::from_utf8(captured.0.lock().expect("the buffer").clone()).expect("UTF-8");
    assert!(!log.contains(&bsn), "the whole log holds the BSN");
    let requests: Vec<serde_json::Value> = log
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|line| line["target"] == ferrohealth_telemetry::request_log::TARGET)
        .collect();
    assert_eq!(3, requests.len(), "one line per request: {log}");
    let first = &requests[0]["fields"];
    assert_eq!("/fhir/Patient/$ihe-pix", first["route"]);
    assert_eq!("GET", first["method"]);
    assert_eq!(503, first["status"]);
    assert_eq!("east-lab", first["caller"]);
    assert!(first["domain"].is_null(), "a query writes to no domain");
    assert!(
        first["request_id"]
            .as_str()
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok()),
        "the server's id"
    );
    assert_eq!(false, first["client_named"]);
    assert!(first["latency_ms"].is_number());
    assert!(
        requests[1]["fields"]["caller"].is_null(),
        "a refused request names no caller"
    );
    assert_eq!(401, requests[1]["fields"]["status"]);
    assert_eq!("<unmatched>", requests[2]["fields"]["route"]);
}
