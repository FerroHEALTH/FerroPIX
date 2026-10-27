// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The HTTP surface on a real socket, inside the stack of
//! `ferrohealth-server`: the health and metrics routes under the base path,
//! the request id, the body limit, the request timeout and the panic catch,
//! each refusal an `OperationOutcome`, and the bounded drain.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the refusals are read as JSON values"
)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::routing::get;
use ferrohealth_health::lifecycle::Lifecycle;
use ferrohealth_health::registry::Registry;
use ferrohealth_server::health::Health;
use ferrohealth_server::request_id;
use ferrohealth_server::section::{BasePath, ServerSection};
use ferrohealth_server::serve::{Drained, ServeError};
use ferrohealth_telemetry::request_log::RequestLog;
use ferrohealth_telemetry::section::TelemetrySection;
use ferrohealth_telemetry::telemetry::{Service, Telemetry};
use ferropix_server::http::router;
use http::StatusCode;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// The section of a server on a free loopback port under `base_path`.
fn settings(base_path: &str) -> ServerSection {
    ServerSection {
        listen: "127.0.0.1:0".parse().expect("a socket address"),
        base_path: BasePath::new(base_path).expect("a base path"),
        body_limit_bytes: 1024,
        request_timeout_ms: 500,
        shutdown_timeout_ms: 300,
        ..ServerSection::default()
    }
}

/// A running server: its address, the switch that stops it, its phase and
/// its task.
struct Running {
    address: SocketAddr,
    stop: oneshot::Sender<()>,
    lifecycle: Lifecycle,
    task: JoinHandle<Result<Drained, ServeError>>,
}

/// Serves `app` on a free loopback port with the drain `section` names.
async fn start(app: Router, section: &ServerSection) -> Running {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let address = listener.local_addr().expect("an address");
    let (stop, stopped) = oneshot::channel::<()>();
    let lifecycle = Lifecycle::default();
    let section = section.clone();
    let phase = lifecycle.clone();
    let task = tokio::spawn(async move {
        ferrohealth_server::serve::run(listener, app, &section, phase, async move {
            stopped.await.unwrap_or(());
        })
        .await
    });
    Running {
        address,
        stop,
        lifecycle,
        task,
    }
}

/// The stack around the one product route `route`, under `section`.
fn product(section: &ServerSection, route: Router, lifecycle: &Lifecycle) -> Router {
    router(
        section,
        Health::new(Registry::default(), lifecycle.clone()),
        Some(route),
        RequestLog::default(),
        None,
    )
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("a client")
}

/// Reads `response` as an `OperationOutcome` and returns its one issue's
/// code and diagnostics.
async fn issue(response: reqwest::Response) -> (String, String) {
    assert_eq!(
        Some("application/fhir+json"),
        response
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
    );
    let outcome: serde_json::Value = response.json().await.expect("a JSON body");
    assert_eq!("OperationOutcome", outcome["resourceType"], "{outcome}");
    let issue = &outcome["issue"][0];
    (
        issue["code"].as_str().unwrap_or_default().to_owned(),
        issue["diagnostics"].as_str().unwrap_or_default().to_owned(),
    )
}

#[tokio::test]
async fn the_health_routes_answer_under_the_base_path_and_nowhere_else() {
    let server = settings("/pix");
    let running = start(
        router(
            &server,
            Health::new(Registry::default(), crate::support::serving()),
            None,
            RequestLog::default(),
            None,
        ),
        &server,
    )
    .await;
    let base = format!("http://{}", running.address);
    for (path, status) in [
        ("/pix/health", StatusCode::OK),
        ("/pix/health/liveness", StatusCode::OK),
        ("/pix/health/readiness", StatusCode::OK),
        ("/health", StatusCode::NOT_FOUND),
    ] {
        let response = client()
            .get(format!("{base}{path}"))
            .send()
            .await
            .expect("the server answers");
        assert_eq!(status, response.status(), "{path}");
    }
    running.stop.send(()).expect("the server is running");
    running
        .task
        .await
        .expect("the task joins")
        .expect("the server stops cleanly");
}

#[tokio::test]
async fn the_metrics_route_exposes_the_request_duration_of_each_route_template() {
    let server = settings("/pix");
    let telemetry = Arc::new(
        Telemetry::new(
            &TelemetrySection::default(),
            Service::new("ferropix", "0.0.0"),
        )
        .expect("the telemetry"),
    );
    let log = RequestLog::default().with_metrics(&telemetry.meter(ferropix_server::METER));
    let running = start(
        router(
            &server,
            Health::new(Registry::default(), crate::support::serving()),
            None,
            log,
            Some(Arc::clone(&telemetry)),
        ),
        &server,
    )
    .await;
    let base = format!("http://{}", running.address);
    let health = client()
        .get(format!("{base}/pix/health"))
        .send()
        .await
        .expect("the server answers");
    assert_eq!(StatusCode::OK, health.status());
    let metrics = client()
        .get(format!("{base}/pix/metrics"))
        .send()
        .await
        .expect("the server answers");
    assert_eq!(StatusCode::OK, metrics.status());
    let text = metrics.text().await.expect("a body");
    assert!(
        text.lines().any(|line| {
            line.starts_with("http_server_request_duration_seconds_count")
                && line.contains(r#"http_route="/pix/health""#)
                && line.contains(r#"http_response_status_code="200""#)
        }),
        "{text}"
    );
    running.stop.send(()).expect("the server is running");
}

#[tokio::test]
async fn a_legal_request_id_is_echoed_and_an_illegal_one_replaced() {
    let server = settings("");
    let running = start(
        router(
            &server,
            Health::new(Registry::default(), crate::support::serving()),
            None,
            RequestLog::default(),
            None,
        ),
        &server,
    )
    .await;
    let url = format!("http://{}/health", running.address);
    let echoed = client()
        .get(&url)
        .header(request_id::HEADER, "corr-42")
        .send()
        .await
        .expect("the server answers");
    assert_eq!(
        Some("corr-42"),
        echoed
            .headers()
            .get(request_id::HEADER)
            .and_then(|v| v.to_str().ok())
    );
    let minted = client()
        .get(&url)
        .header(request_id::HEADER, "x".repeat(request_id::MAX_LENGTH + 1))
        .send()
        .await
        .expect("the server answers");
    let id = minted
        .headers()
        .get(request_id::HEADER)
        .and_then(|v| v.to_str().ok())
        .expect("a minted id");
    assert!(
        uuid::Uuid::parse_str(id).is_ok(),
        "a UUID replaces the illegal value: {id}"
    );
    running.stop.send(()).expect("the server is running");
}

#[tokio::test]
async fn a_body_over_the_limit_is_answered_413_with_an_operation_outcome() {
    let server = settings("");
    let lifecycle = Lifecycle::default();
    let app = product(
        &server,
        Router::new().route(
            "/echo",
            axum::routing::post(|body: String| async move { body }),
        ),
        &lifecycle,
    );
    let running = start(app, &server).await;
    let url = format!("http://{}/echo", running.address);
    let small = client()
        .post(&url)
        .body("a".repeat(server.body_limit_bytes))
        .send()
        .await
        .expect("the server answers");
    assert_eq!(StatusCode::OK, small.status());
    let large = client()
        .post(&url)
        .body("a".repeat(server.body_limit_bytes + 1))
        .send()
        .await
        .expect("the server answers");
    assert_eq!(StatusCode::PAYLOAD_TOO_LARGE, large.status());
    assert_eq!("too-long", issue(large).await.0);
    running.stop.send(()).expect("the server is running");
}

#[tokio::test]
async fn a_request_past_its_timeout_is_answered_408_with_an_operation_outcome() {
    let server = settings("");
    let lifecycle = Lifecycle::default();
    let app = product(
        &server,
        Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                "late"
            }),
        ),
        &lifecycle,
    );
    let running = start(app, &server).await;
    let response = client()
        .get(format!("http://{}/slow", running.address))
        .send()
        .await
        .expect("the server answers");
    assert_eq!(StatusCode::REQUEST_TIMEOUT, response.status());
    assert_eq!("timeout", issue(response).await.0);
    running.stop.send(()).expect("the server is running");
}

#[tokio::test]
async fn a_panicking_handler_is_a_500_naming_the_server_s_request_id_and_not_the_message() {
    let server = settings("");
    let lifecycle = Lifecycle::default();
    let app = product(&server, Router::new().route("/boom", get(boom)), &lifecycle);
    let running = start(app, &server).await;
    let response = client()
        .get(format!("http://{}/boom", running.address))
        .header(request_id::HEADER, "corr-boom")
        .send()
        .await
        .expect("the server answers");
    assert_eq!(StatusCode::INTERNAL_SERVER_ERROR, response.status());
    assert_eq!(
        Some("corr-boom"),
        response
            .headers()
            .get(request_id::HEADER)
            .and_then(|v| v.to_str().ok()),
        "the client's id is echoed"
    );
    let (code, diagnostics) = issue(response).await;
    assert_eq!("exception", code);
    let server_id = diagnostics
        .strip_prefix("the server could not complete the request (request id ")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_else(|| panic!("the diagnostics: {diagnostics}"));
    assert!(
        uuid::Uuid::parse_str(server_id).is_ok(),
        "the body names the server's id, which the log carries: {diagnostics}"
    );
    assert!(!diagnostics.contains("corr-boom"), "{diagnostics}");
    assert!(!diagnostics.contains("must not carry"), "{diagnostics}");
    running.stop.send(()).expect("the server is running");
}

#[tokio::test]
async fn readiness_answers_503_while_the_drain_delay_runs() {
    let mut server = settings("");
    server.drain_delay_ms = 1_000;
    let lifecycle = Lifecycle::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let address = listener.local_addr().expect("an address");
    let app = product(&server, Router::new(), &lifecycle);
    let (stop, stopped) = oneshot::channel::<()>();
    let phase = lifecycle.clone();
    let section = server.clone();
    let task = tokio::spawn(async move {
        ferrohealth_server::serve::run(listener, app, &section, phase, async move {
            stopped.await.unwrap_or(());
        })
        .await
    });
    let url = format!("http://{address}/health/readiness");
    let ready = client().get(&url).send().await.expect("the server answers");
    assert_eq!(StatusCode::OK, ready.status());
    stop.send(()).expect("the server is running");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let draining = client()
        .get(&url)
        .send()
        .await
        .expect("the listener still accepts");
    assert_eq!(StatusCode::SERVICE_UNAVAILABLE, draining.status());
    assert_eq!(
        Ok(Drained::Complete),
        task.await
            .expect("the task joins")
            .map_err(|error| error.to_string())
    );
}

#[tokio::test]
async fn the_drain_is_bounded_and_cuts_a_request_that_outlasts_it() {
    let mut server = settings("");
    server.request_timeout_ms = 30_000;
    let running_phase = Lifecycle::default();
    let app = product(
        &server,
        Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(20)).await;
                "late"
            }),
        ),
        &running_phase,
    );
    let running = start(app, &server).await;
    let url = format!("http://{}/slow", running.address);
    let in_flight = tokio::spawn(async move { client().get(url).send().await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let asked = Instant::now();
    running.stop.send(()).expect("the server is running");
    let drained = running
        .task
        .await
        .expect("the task joins")
        .expect("the drain ends");
    assert_eq!(Drained::Cut, drained);
    let took = asked.elapsed();
    assert!(
        took < Duration::from_secs(5),
        "the drain is bounded by shutdown_timeout_ms: {took:?}"
    );
    in_flight.abort();
}

#[tokio::test]
async fn the_drain_lets_a_short_request_finish() {
    let mut server = settings("");
    server.shutdown_timeout_ms = 5_000;
    let running_phase = Lifecycle::default();
    let app = product(
        &server,
        Router::new().route(
            "/short",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(300)).await;
                "done"
            }),
        ),
        &running_phase,
    );
    let running = start(app, &server).await;
    let url = format!("http://{}/short", running.address);
    let in_flight = tokio::spawn(async move { client().get(url).send().await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    running.stop.send(()).expect("the server is running");
    let response = in_flight
        .await
        .expect("the request task joins")
        .expect("the request in flight is answered");
    assert_eq!(StatusCode::OK, response.status());
    assert_eq!("done", response.text().await.expect("a body"));
    assert_eq!(
        Drained::Complete,
        running
            .task
            .await
            .expect("the task joins")
            .expect("the server stops cleanly")
    );
    assert_eq!(
        ferrohealth_health::document::Phase::Draining,
        running.lifecycle.phase()
    );
}

/// A handler that panics over a value the log must not carry.
async fn boom() -> &'static str {
    panic!("a value the log must not carry")
}
