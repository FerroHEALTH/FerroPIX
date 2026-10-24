// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FHIR face under `{base}/fhir` and the operator API under
//! `{base}/operator`: every route of [`crate::auth::table`] the
//! configuration switches on, behind the guard.
//!
//! The capability statement, the SMART discovery document, the PIXm
//! Manager ([`pixm`]) and the PDQm Supplier ([`pdqm`]) answer; every other
//! transaction route answers `501` with a `not-supported` issue until the
//! issue that serves it lands, so the guard in front of it is in place and
//! tested first. [`status`] is the one table of every outcome a transaction
//! answers, and [`wire`] puts each in JSON or XML.

pub mod capability;
pub mod outcome;
pub mod pdqm;
pub mod pixm;
pub mod status;
pub mod wire;

use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodFilter, on};
use ferropix_operator_api::health::Dependencies;
use http::StatusCode;

use crate::audit::Audit;
use crate::audit::reserve::{self, Reserving};
use crate::auth::guard::{self, Confine};
use crate::auth::table::{self, Mount};
use crate::auth::{Discovery, Smart};
use crate::config::section::matching::Matching;
use crate::config::section::operator::Operator;
use crate::config::section::profiles::{Deprecated, Pdqm, Pixm, Pmir};
use crate::fhir::outcome::{IssueType, body, respond};
use crate::health::Registry;
use crate::store::Store;

/// What the transactions of the FHIR face read and write: the store, the
/// `[matching]` section the feed and the scorer run with, and how the PDQm
/// Supplier answers a merged person.
#[derive(Debug, Clone)]
pub struct Backend {
    /// The cross-reference store.
    pub store: Store,
    /// The `[matching]` section, resolved.
    pub matching: Matching,
    /// `[pdqm] deprecated`.
    pub deprecated: Deprecated,
}

/// Which switches of the configuration are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each field is one switch of the configuration, read once at start"
)]
pub struct Mounts {
    /// `[pixm] enabled`.
    pub pixm: bool,
    /// `[pixm] remove_patient`.
    pub remove_patient: bool,
    /// `[pdqm] enabled`.
    pub pdqm: bool,
    /// `[pmir] enabled`.
    pub pmir: bool,
    /// `[operator] enabled`.
    pub operator: bool,
}

impl Mounts {
    /// Returns every switch on.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            pixm: true,
            remove_patient: true,
            pdqm: true,
            pmir: true,
            operator: true,
        }
    }

    /// Returns the switches the `[pixm]`, `[pdqm]`, `[pmir]` and `[operator]`
    /// sections set.
    #[must_use]
    pub const fn of(pixm: &Pixm, pdqm: &Pdqm, pmir: &Pmir, operator: &Operator) -> Self {
        Self {
            pixm: pixm.enabled,
            remove_patient: pixm.remove_patient,
            pdqm: pdqm.enabled,
            pmir: pmir.enabled,
            operator: operator.enabled,
        }
    }

    /// Returns whether `mount` is switched on.
    #[must_use]
    pub const fn admits(self, mount: Mount) -> bool {
        match mount {
            Mount::Always => true,
            Mount::Pixm => self.pixm,
            Mount::PixmRemove => self.pixm && self.remove_patient,
            Mount::Pdqm => self.pdqm,
            Mount::Pmir => self.pmir,
            Mount::Operator => self.operator,
        }
    }
}

/// Returns the guarded routes `mounts` switches on, for a server mounted
/// under `base` and reached at `public_base_url`, its transactions over
/// `backend`.
///
/// The router is meant to be nested under `base` with the health routes;
/// the guard reads the matched route templates under it.
pub fn routes(
    base: &str,
    public_base_url: &url::Url,
    mounts: Mounts,
    smart: &Arc<Smart>,
    health: &Registry,
    audit: &Audit,
    backend: &Backend,
) -> Router {
    let statement = Arc::new(capability::statement(public_base_url, smart, mounts));
    let configuration = smart.configuration().map(Arc::new);
    let manager = Arc::new(pixm::Manager::new(
        &backend.store,
        &backend.matching,
        audit,
        smart,
        base,
        public_base_url,
    ));
    let pdqm = Arc::new(pdqm::Face::new(
        Arc::new(crate::pdqm::Supplier::new(
            backend.store.clone(),
            &backend.matching,
            backend.deprecated,
            public_base_url,
        )),
        audit.clone(),
    ));
    let mut router = Router::new();
    for route in table::ROUTES
        .iter()
        .filter(|route| mounts.admits(route.mount))
    {
        let Ok(filter) = MethodFilter::try_from(route.method.clone()) else {
            continue;
        };
        if let Some(handler) = pdqm::handler(route, filter, &pdqm) {
            router = router.route(route.path, handler);
            continue;
        }
        let handler = match route.interaction {
            "capabilities" => {
                let statement = Arc::clone(&statement);
                on(filter, move |request: Request| {
                    let statement = Arc::clone(&statement);
                    async move { metadata(&statement, &request) }
                })
            }
            "ITI-83" | "ITI-104" => pixm_route(&route.method, filter, &manager),
            "smart-configuration" => {
                let configuration = configuration.clone();
                on(filter, move || {
                    let configuration = configuration.clone();
                    async move { discovery(configuration.as_deref()) }
                })
            }
            "operator-dependencies" => {
                let (health, audit) = (health.clone(), audit.clone());
                on(filter, move || dependencies(health.clone(), audit.clone()))
            }
            _ if route.mount == Mount::Operator => on(filter, pending_operator),
            _ => on(filter, pending_fhir),
        };
        router = router.route(route.path, handler);
    }
    router
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(Reserving::new(base, audit.clone())),
            reserve::layer,
        ))
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(Confine::new(base, Arc::clone(smart))),
            guard::confine,
        ))
        .layer(guard::layer(base, smart))
}

/// `GET {base}/fhir/metadata`: the statement in the format the request
/// negotiates, JSON or XML, or `406` (FHIR R4 http.html §3.1.0.1.10).
fn metadata(
    statement: &fhir_types::r4::capability_statement::CapabilityStatement,
    request: &Request,
) -> Response {
    match wire::negotiate(request.uri().query(), request.headers()) {
        Ok(format) => wire::send_resource(statement, format),
        Err(refused) => *refused,
    }
}

/// The handler of the PIXm route `method`: ITI-83 by `GET` or `POST`,
/// ITI-104's conditional update by `PUT` and its conditional delete by
/// `DELETE`.
fn pixm_route(
    method: &http::Method,
    filter: MethodFilter,
    manager: &Arc<pixm::Manager>,
) -> axum::routing::MethodRouter {
    let manager = Arc::clone(manager);
    match *method {
        http::Method::PUT => on(filter, move |request: Request| {
            let manager = Arc::clone(&manager);
            async move { manager.update(request).await }
        }),
        http::Method::DELETE => on(filter, move |request: Request| {
            let manager = Arc::clone(&manager);
            async move { manager.remove(request).await }
        }),
        _ => on(filter, move |request: Request| {
            let manager = Arc::clone(&manager);
            async move { manager.query(request).await }
        }),
    }
}

/// `GET {base}/fhir/.well-known/smart-configuration`: the SMART discovery
/// document, which "SHALL be JSON, regardless of `Accept` headers" (SMART
/// App Launch 2.2.0, Conformance, §Using Well-Known), or `404` on a surface
/// of Basic users alone.
fn discovery(configuration: Option<&Discovery>) -> Response {
    match configuration {
        Some(document) => body(StatusCode::OK, document, "application/json"),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `GET {base}/operator/dependencies`: the readiness report with the last
/// outcome of every upstream, behind the steward scope so an anonymous
/// reader learns no endpoint (FerroFED's decision A83; no specification
/// governs this: our own design).
// TODO(#104): name every PMIR subscriber with its last outcome.
async fn dependencies(health: Registry, audit: Audit) -> Response {
    let report = Dependencies {
        readiness: health.evaluate().await,
        upstreams: audit.upstream().into_iter().collect(),
    };
    body(StatusCode::OK, &report, "application/json")
}

/// A transaction of the FHIR face that is not served yet.
// TODO(#101): serve ITI-93 and ITI-94.
async fn pending_fhir() -> Response {
    respond(
        StatusCode::NOT_IMPLEMENTED,
        IssueType::NotSupported,
        "FerroPIX does not serve this interaction yet",
        None,
    )
}

/// A route of the operator API that is not served yet.
// TODO(#90): serve the operator API.
async fn pending_operator() -> Response {
    respond(
        StatusCode::NOT_IMPLEMENTED,
        IssueType::NotSupported,
        "FerroPIX does not serve this route yet",
        None,
    )
}
