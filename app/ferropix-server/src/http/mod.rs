// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The HTTP surface: the routes under `{base}` inside the family stack of
//! `ferrohealth-server`.
//!
//! The stack carries the request id, the panic catch, the request timeout,
//! the body limit, the overload bound, the compression and the request log
//! of `ferrohealth-telemetry`, so every request's line names the server's id
//! and never the client's. Every body the stack writes itself, a panic and a
//! refusal, is a FHIR `OperationOutcome` under the FHIR face and the XCPD
//! gateway and an RFC 9457 problem elsewhere ([`policy`]). The bounded drain is
//! `ferrohealth_server::serve::run`'s.
//!
//! No specification governs the process model: our own design, in the
//! family's shape.

use std::sync::Arc;

use axum::Router;
use ferrohealth_server::health::Health;
use ferrohealth_server::section::ServerSection;
use ferrohealth_server::stack::{self, StackPolicy};
use ferrohealth_telemetry::request_log::{RequestId, RequestLog};
use ferrohealth_telemetry::telemetry::{Telemetry, prometheus_route};
use http::request::Parts;

use crate::problem;

/// The path of the Prometheus exposition under `{base}`.
pub const METRICS: &str = "/metrics";

/// Builds the HTTP application under `server`'s base path, inside the stack
/// of `ferrohealth-server` with `log` as the request log.
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
    server: &ServerSection,
    health: Health,
    guarded: Option<Router>,
    log: RequestLog,
    metrics: Option<Arc<Telemetry>>,
) -> Router {
    let mut routes = Router::new();
    if let Some(telemetry) = metrics {
        routes = routes.route(METRICS, prometheus_route(telemetry));
    }
    if let Some(guarded) = guarded {
        routes = routes.merge(guarded);
    }
    stack::app(
        routes,
        health,
        server,
        policy(server.base_path.as_str(), log),
    )
}

/// The policy of the stack under `base`: a panic and every refusal the
/// stack writes answer an `OperationOutcome` under the FHIR face and the
/// XCPD gateway and a problem elsewhere, and `log` is the request log.
#[must_use]
pub fn policy(base: &str, log: RequestLog) -> StackPolicy {
    StackPolicy {
        panic_body: problem::panic_body(base),
        refusal_body: problem::refusal_body(base),
        request_log: log,
    }
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
