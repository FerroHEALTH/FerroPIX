// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! One log line per request: the request id, the matched route template,
//! the method, the status, the latency, the caller and the domain a feed
//! wrote to (`docs/architecture.md` §9).
//!
//! The raw path, the query string, a body and a header value never reach the
//! line: the ITI-83 `GET` form carries the identifier in its query, and the
//! line names `/fhir/Patient/$ihe-pix` and nothing after it (#52). The
//! caller and the domain are left on the response by the guard behind which
//! they are known.
//!
//! The request runs inside a `request` span with the method, the matched
//! route template and the status, the span `[telemetry] otlp_endpoint`
//! exports; the caller, the domain and the request id stay on the log line.
//! No specification governs the log line or the span: our own design.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;
use tracing::Instrument as _;

use crate::http::request_id;
use crate::usage_report::window::Window;

/// The `tracing` target of the request log.
pub const TARGET: &str = "ferropix_server::request";

/// The route a request no route matched is logged under: its raw path may
/// carry an identifier, so no part of it is logged.
pub const UNMATCHED: &str = "<unmatched>";

/// Who called, as the guard admitted the credential: the token's
/// `client_id`, or its subject or the Basic user-id when it names none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller(pub String);

/// The identifier domains a feed wrote to, by system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written(pub Vec<String>);

/// Logs `request` after it completes and returns its response untouched.
///
/// When the request carries the usage report's [`Window`] as an extension,
/// the matched route template, the status and the latency are recorded into
/// it too.
pub async fn log(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let matched = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_owned());
    let window = request.extensions().get::<Arc<Window>>().cloned();
    let request_id = request_id::of(request.headers())
        .unwrap_or_default()
        .to_owned();
    let span = tracing::info_span!(
        target: TARGET,
        "request",
        http.request.method = method.as_str(),
        http.route = matched.as_deref().unwrap_or(UNMATCHED),
        http.response.status_code = tracing::field::Empty,
    );
    let started = Instant::now();
    let response = next.run(request).instrument(span.clone()).await;
    let elapsed = started.elapsed();
    let latency_ms = elapsed.as_secs_f64() * 1000.0;
    let status = response.status();
    span.record("http.response.status_code", status.as_u16());
    if let Some(window) = window {
        window.record(&method, matched.as_deref(), status, elapsed);
    }
    let route = matched.unwrap_or_else(|| UNMATCHED.to_owned());
    let caller = response
        .extensions()
        .get::<Caller>()
        .map_or("-", |caller| caller.0.as_str());
    let domain = response
        .extensions()
        .get::<Written>()
        .map_or_else(|| String::from("-"), |written| written.0.join(" "));
    let (method, route, request_id, domain) = (
        method.as_str(),
        route.as_str(),
        request_id.as_str(),
        domain.as_str(),
    );
    let code = status.as_u16();
    if status.is_server_error() {
        tracing::error!(target: TARGET, request_id, method, route, status = code, latency_ms, caller, domain, "request");
    } else if status.is_client_error() {
        tracing::warn!(target: TARGET, request_id, method, route, status = code, latency_ms, caller, domain, "request");
    } else {
        tracing::info!(target: TARGET, request_id, method, route, status = code, latency_ms, caller, domain, "request");
    }
    response
}
