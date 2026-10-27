// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The HTTP surface on a real socket: the health and metrics routes under the
//! base path, the request id, the body ceiling, the request timeout, the panic catch and
//! the bounded drain.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::routing::get;
use ferrohealth_server::request_id;
use ferrohealth_telemetry::request_log::RequestLog;
use ferrohealth_telemetry::section::TelemetrySection;
use ferrohealth_telemetry::telemetry::{Service, Telemetry};
use ferropix_server::config::section::server::ServerSettings;
use ferropix_server::health::Registry;
use ferropix_server::http::{router, serve_until, with_middleware};
use http::StatusCode;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// The settings of a server on a free loopback port under `base_path`.
fn settings(base_path: &str) -> ServerSettings {
    ServerSettings {
        listen: "127.0.0.1:0".parse().expect("a socket address"),
        base_path: base_path.to_owned(),
        public_base_url: url::Url::parse("http://localhost:8080").expect("a URL"),
        body_limit: 1024,
        request_timeout: Duration::from_millis(500),
        drain_timeout: Duration::from_millis(300),
    }
}

/// A running server: its address, the switch that stops it and its task.
struct Running {
    address: SocketAddr,
    stop: oneshot::Sender<()>,
    task: JoinHandle<std::io::Result<()>>,
}

/// Serves `app` on a free loopback port with `drain` as the drain window.
async fn start(app: Router, drain: Duration) -> Running {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let address = listener.local_addr().expect("an address");
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(serve_until(listener, app, drain, async move {
        stopped.await.unwrap_or(());
    }));
    Running {
        address,
        stop,
        task,
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("a client")
}

#[tokio::test]
async fn the_health_routes_answer_under_the_base_path_and_nowhere_else() {
    let server = settings("/pix");
    let running = start(
        router(
            &server,
            &Registry::default(),
            None,
            RequestLog::default(),
            None,
        ),
        server.drain_timeout,
    )
    .await;
    let base = format!("http://{}", running.address);
    for (path, status) in [
        ("/pix/health", StatusCode::OK),
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
            &Registry::default(),
            None,
            log,
            Some(Arc::clone(&telemetry)),
        ),
        server.drain_timeout,
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
            &Registry::default(),
            None,
            RequestLog::default(),
            None,
        ),
        server.drain_timeout,
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
async fn a_body_over_the_ceiling_is_answered_413() {
    let server = settings("");
    let app = with_middleware(
        Router::new().route(
            "/echo",
            axum::routing::post(|body: String| async move { body }),
        ),
        &server,
        RequestLog::default(),
    );
    let running = start(app, server.drain_timeout).await;
    let url = format!("http://{}/echo", running.address);
    let small = client()
        .post(&url)
        .body("a".repeat(server.body_limit))
        .send()
        .await
        .expect("the server answers");
    assert_eq!(StatusCode::OK, small.status());
    let large = client()
        .post(&url)
        .body("a".repeat(server.body_limit + 1))
        .send()
        .await
        .expect("the server answers");
    assert_eq!(StatusCode::PAYLOAD_TOO_LARGE, large.status());
    running.stop.send(()).expect("the server is running");
}

#[tokio::test]
async fn a_request_past_its_timeout_is_answered_408() {
    let server = settings("");
    let app = with_middleware(
        Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                "late"
            }),
        ),
        &server,
        RequestLog::default(),
    );
    let running = start(app, server.drain_timeout).await;
    let response = client()
        .get(format!("http://{}/slow", running.address))
        .send()
        .await
        .expect("the server answers");
    assert_eq!(StatusCode::REQUEST_TIMEOUT, response.status());
    running.stop.send(()).expect("the server is running");
}

#[tokio::test]
async fn a_panicking_handler_is_a_500_naming_the_server_s_request_id_and_not_the_message() {
    let server = settings("");
    let app = with_middleware(
        Router::new().route("/boom", get(boom)),
        &server,
        RequestLog::default(),
    );
    let running = start(app, server.drain_timeout).await;
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
    let body = response.text().await.expect("a body");
    let server_id = body
        .strip_prefix(r#"{"error":"internal","request_id":""#)
        .and_then(|rest| rest.strip_suffix(r#""}"#))
        .unwrap_or_else(|| panic!("the internal body: {body}"));
    assert!(
        uuid::Uuid::parse_str(server_id).is_ok(),
        "the body names the server's id, which the log carries: {body}"
    );
    assert!(!body.contains("boom"), "{body}");
    running.stop.send(()).expect("the server is running");
}

#[tokio::test]
async fn the_drain_is_bounded_and_drops_a_request_that_outlasts_it() {
    let mut server = settings("");
    server.request_timeout = Duration::from_secs(30);
    let app = with_middleware(
        Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(20)).await;
                "late"
            }),
        ),
        &server,
        RequestLog::default(),
    );
    let drain = Duration::from_millis(300);
    let running = start(app, drain).await;
    let url = format!("http://{}/slow", running.address);
    let in_flight = tokio::spawn(async move { client().get(url).send().await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let asked = Instant::now();
    running.stop.send(()).expect("the server is running");
    running
        .task
        .await
        .expect("the task joins")
        .expect("the drain ends cleanly");
    let took = asked.elapsed();
    assert!(
        took < Duration::from_secs(5),
        "the drain is bounded by its window: {took:?}"
    );
    in_flight.abort();
}

#[tokio::test]
async fn the_drain_lets_a_short_request_finish() {
    let server = settings("");
    let app = with_middleware(
        Router::new().route(
            "/short",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(300)).await;
                "done"
            }),
        ),
        &server,
        RequestLog::default(),
    );
    let running = start(app, Duration::from_secs(5)).await;
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
    running
        .task
        .await
        .expect("the task joins")
        .expect("the server stops cleanly");
}

/// A handler that panics over a value the log must not carry.
async fn boom() -> &'static str {
    panic!("a value the log must not carry")
}
