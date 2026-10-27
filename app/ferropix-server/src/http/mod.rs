// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The HTTP surface: the routes under `{base}`, the family's middleware and
//! the bounded drain on `SIGTERM`.
//!
//! The request id is `ferrohealth-server`'s and the request log is
//! `ferrohealth-telemetry`'s, so every request's line names the server's id
//! and never the client's.
//!
//! No specification governs the process model: our own design, after
//! FerroBRIDGE's and FerroFED's servers.

pub mod panic;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use ferrohealth_server::health::Health;
use ferrohealth_telemetry::request_log::{RequestId, RequestLog};
use ferrohealth_telemetry::telemetry::{Telemetry, prometheus_route};
use http::StatusCode;
use http::request::Parts;
use tokio::net::TcpListener;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

use crate::config::section::server::ServerSettings;

/// The path of the Prometheus exposition under `{base}`.
pub const METRICS: &str = "/metrics";

/// Builds the HTTP application under `server`'s base path, with the shared
/// middleware and `log` as the request log.
///
/// The routes are the health routes of `ferrohealth-server` over `health`,
/// the Prometheus exposition of `metrics` when given, and the `guarded`
/// routes of the FHIR face and the operator API when given.
///
/// `GET {base}/health/liveness` answers `200` while the process serves,
/// `GET {base}/health/readiness` answers the readiness document, `200` while
/// the process serves and no indicator is down and `503` otherwise, naming
/// each indicator and never its detail, `GET {base}/health` answers the same
/// status with the state alone, and `GET {base}/metrics` answers every
/// metric in the Prometheus text format.
pub fn router(
    server: &ServerSettings,
    health: Health,
    guarded: Option<Router>,
    log: RequestLog,
    metrics: Option<Arc<Telemetry>>,
) -> Router {
    let mut routes = ferrohealth_server::health::routes(health);
    if let Some(telemetry) = metrics {
        routes = routes.route(METRICS, prometheus_route(telemetry));
    }
    if let Some(guarded) = guarded {
        routes = routes.merge(guarded);
    }
    let mounted = if server.base_path.is_empty() {
        routes
    } else {
        Router::new().nest(&server.base_path, routes)
    };
    with_middleware(mounted, server, log)
}

/// Applies the middleware every FerroPIX route carries to `router`, with
/// `log` as the request log.
///
/// Outermost first: the panic renderer, the request id of
/// `ferrohealth-server`, the panic catcher, the request timeout, the body
/// ceiling and the request log. The renderer sits outside the request id
/// because it reads the server's id from the response that layer has just
/// marked; the request log runs after routing, so it reads the matched route
/// template, and inside the request id, so it reads the server's id.
pub fn with_middleware(router: Router, server: &ServerSettings, log: RequestLog) -> Router {
    router
        .layer(log)
        .layer(RequestBodyLimitLayer::new(server.body_limit))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            server.request_timeout,
        ))
        .layer(CatchPanicLayer::custom(panic::caught))
        .layer(axum::middleware::from_fn(
            ferrohealth_server::request_id::assign,
        ))
        .layer(axum::middleware::from_fn(panic::render))
}

/// Returns the `X-Request-Id` of the exchange `parts` belongs to, as the
/// response carries it: the client's when it sent a legal one, else the
/// server's own.
///
/// The audit trail records this value (IHE BALP 1.1.4, `BasicAuditEntityType`
/// `XrequestId`: "the value of X-Request-Id").
#[must_use]
pub fn exchanged_request_id(parts: &Parts) -> Option<&str> {
    ferrohealth_server::request_id::of_client(&parts.headers)
        .or_else(|| parts.extensions.get::<RequestId>().map(RequestId::id))
}

/// Serves `app` on an already-bound listener until `shutdown` completes, then
/// finishes the requests in flight within `drain`.
///
/// A container runtime stops a container with `SIGTERM` to PID 1 and kills it
/// after a grace period, so the drain is bounded: a connection still open when
/// `drain` elapses is dropped and the function returns.
///
/// # Errors
/// Returns the I/O error from accepting or serving connections.
pub async fn serve_until<F>(
    listener: TcpListener,
    app: Router,
    drain: Duration,
    shutdown: F,
) -> std::io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let signalled = Arc::new(tokio::sync::Notify::new());
    let inner = Arc::clone(&signalled);
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown.await;
        inner.notify_one();
    })
    .into_future();
    let mut server = std::pin::pin!(server);
    tokio::select! {
        result = &mut server => return result,
        () = signalled.notified() => {}
    }
    if let Ok(result) = tokio::time::timeout(drain, server).await {
        return result;
    }
    tracing::warn!(
        drain_ms = drain.as_millis(),
        "the drain did not finish in time; the remaining connections are dropped"
    );
    Ok(())
}

/// Completes when the process receives `SIGTERM` or `SIGINT`.
///
/// A failure to install a handler is logged and that arm never completes, so
/// the server keeps serving and the runtime's own kill stays the backstop.
pub async fn shutdown_signal() {
    let interrupt = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "cannot listen for SIGINT");
            std::future::pending::<()>().await;
        }
    };
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        () = interrupt => tracing::info!("SIGINT received, draining"),
        () = terminate => tracing::info!("SIGTERM received, draining"),
    }
}
