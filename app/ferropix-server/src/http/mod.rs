// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The HTTP surface: the routes under `{base}`, the family's middleware and
//! the bounded drain on `SIGTERM`.
//!
//! No specification governs the process model: our own design, after
//! FerroBRIDGE's and FerroFED's servers.

pub mod panic;
pub mod request_id;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::routing::get;
use http::StatusCode;
use tokio::net::TcpListener;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;

use crate::config::section::server::ServerSettings;

/// The path of the liveness route under `{base}`.
pub const LIVENESS: &str = "/health";

/// The path of the readiness route under `{base}`.
pub const READINESS: &str = "/health/readiness";

/// Builds the HTTP application under `server`'s base path, with the shared
/// middleware: the health routes, and the `guarded` routes of the FHIR face
/// and the operator API when given.
///
/// `GET {base}/health` answers `200` while the process is up, and
/// `GET {base}/health/readiness` answers `200` while it can serve.
pub fn router(server: &ServerSettings, guarded: Option<Router>) -> Router {
    let mut routes = Router::new()
        .route(LIVENESS, get(liveness))
        .route(READINESS, get(readiness));
    if let Some(guarded) = guarded {
        routes = routes.merge(guarded);
    }
    let mounted = if server.base_path.is_empty() {
        routes
    } else {
        Router::new().nest(&server.base_path, routes)
    };
    with_middleware(mounted, server)
}

/// Applies the middleware every FerroPIX route carries to `router`.
///
/// Outermost first: the request-id normalizer, the layer that mints one, the
/// panic renderer, the layer that propagates the id onto the response, the
/// panic catcher, the request timeout and the body ceiling. The renderer sits
/// outside the propagate layer because it reads the id from the response that
/// layer has just stamped.
pub fn with_middleware(router: Router, server: &ServerSettings) -> Router {
    router
        .layer(RequestBodyLimitLayer::new(server.body_limit))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            server.request_timeout,
        ))
        .layer(CatchPanicLayer::custom(panic::caught))
        .layer(PropagateRequestIdLayer::new(request_id::HEADER))
        .layer(axum::middleware::from_fn(panic::render))
        .layer(SetRequestIdLayer::new(request_id::HEADER, request_id::Mint))
        .layer(axum::middleware::map_request(request_id::strip_illegal))
}

/// `GET {base}/health`: `200` while the process is up.
async fn liveness() -> StatusCode {
    StatusCode::OK
}

/// `GET {base}/health/readiness`: `200` while the server can serve.
async fn readiness() -> StatusCode {
    // TODO(#64): answer from the indicator registry, 503 naming each indicator that is down.
    StatusCode::OK
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
    let server = axum::serve(listener, app)
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
