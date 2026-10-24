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
use std::time::Duration;

use ferropix_operator_api::health::{IndicatorState, State};
use ferropix_server::config::section::server::ServerSettings;
use ferropix_server::health::{Check, HealthIndicator, Registry};
use ferropix_server::http::{router, serve_until};
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
                State::Up => IndicatorState::up(),
                State::Down => IndicatorState::down("the stub is down"),
            }
        })
    }
}

/// The settings of a server on a free loopback port at the root.
fn server() -> ServerSettings {
    ServerSettings {
        listen: "127.0.0.1:0".parse().expect("a socket address"),
        base_path: String::new(),
        public_base_url: url::Url::parse("http://localhost:8080").expect("a URL"),
        body_limit: 1024,
        request_timeout: Duration::from_secs(5),
        drain_timeout: Duration::from_millis(300),
    }
}

/// Returns a registry of the indicators named, each `Up` or `Down`.
fn registry(indicators: &[(&'static str, State)]) -> Registry {
    indicators
        .iter()
        .fold(Registry::default(), |registry, (name, state)| {
            registry.and(Arc::new(Fixed(name, *state)))
        })
}

#[tokio::test]
async fn readiness_answers_503_naming_each_down_indicator_and_200_otherwise() {
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
        let app = router(&server, &registry(&indicators), None);
        let answer = send(&app, Method::GET, "/health/readiness", None).await;
        assert_eq!(expected, answer.status, "{indicators:?}");
        let document = answer.json();
        let named: Vec<&str> = document["indicators"]
            .as_object()
            .expect("the indicators are an object")
            .iter()
            .filter(|(_, state)| state["state"] == "down")
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(down, named, "{indicators:?}");
        let aggregate = if down.is_empty() { "up" } else { "down" };
        assert_eq!(aggregate, document["state"], "{indicators:?}");
        let live = send(&app, Method::GET, "/health", None).await;
        assert_eq!(StatusCode::OK, live.status, "liveness asks no indicator");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn healthcheck_exits_one_on_a_server_whose_readiness_answers_503() {
    let secrets = Secrets::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let port = listener.local_addr().expect("an address").port();
    let app = router(&server(), &registry(&[("database", State::Down)]), None);
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(serve_until(
        listener,
        app,
        Duration::from_millis(300),
        async move {
            stopped.await.unwrap_or(());
        },
    ));
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
    assert!(!answer.status.is_server_error() || answer.status == StatusCode::NOT_IMPLEMENTED);
    let anonymous = send(&app, Method::GET, &uri, None).await;
    assert_eq!(StatusCode::UNAUTHORIZED, anonymous.status);
    let unmatched = format!("/fhir/Patient/{bsn}/_history?x={bsn}");
    send(&app, Method::GET, &unmatched, Some(&token)).await;
    let log = String::from_utf8(captured.0.lock().expect("the buffer").clone()).expect("UTF-8");
    assert!(!log.contains(&bsn), "the whole log holds the BSN");
    let requests: Vec<serde_json::Value> = log
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|line| line["target"] == "ferropix_server::request")
        .collect();
    assert_eq!(3, requests.len(), "one line per request: {log}");
    let first = &requests[0]["fields"];
    assert_eq!("/fhir/Patient/$ihe-pix", first["route"]);
    assert_eq!("GET", first["method"]);
    assert_eq!(501, first["status"]);
    assert_eq!("east-lab", first["caller"]);
    assert_eq!("-", first["domain"]);
    assert!(
        first["request_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert!(first["latency_ms"].is_number());
    assert_eq!("-", requests[1]["fields"]["caller"], "a refused caller");
    assert_eq!(401, requests[1]["fields"]["status"]);
    assert_eq!("<unmatched>", requests[2]["fields"]["route"]);
}
