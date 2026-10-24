// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FHIR face under `{base}/fhir` and the operator API under
//! `{base}/operator`: every route of [`crate::auth::table`] the
//! configuration switches on, behind the guard.
//!
//! The capability statement and the SMART discovery document answer today;
//! every transaction route answers `501` with a `not-supported` issue until
//! the issue that serves it lands, so the guard in front of it is in place
//! and tested first.

pub mod capability;
pub mod outcome;

use std::sync::Arc;

use axum::Router;
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodFilter, on};
use ferropix_operator_api::health::Dependencies;
use http::StatusCode;

use crate::audit::Audit;
use crate::audit::reserve::{self, Reserving};
use crate::auth::guard::{self, Confine};
use crate::auth::table::{self, Mount};
use crate::auth::{Discovery, Smart};
use crate::fhir::outcome::{FHIR_JSON, IssueType, body, respond};
use crate::health::Registry;

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
/// under `base` and reached at `public_base_url`.
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
) -> Router {
    let statement = Arc::new(capability::statement(public_base_url, smart));
    let configuration = smart.configuration().map(Arc::new);
    let mut router = Router::new();
    for route in table::ROUTES
        .iter()
        .filter(|route| mounts.admits(route.mount))
    {
        let Ok(filter) = MethodFilter::try_from(route.method.clone()) else {
            continue;
        };
        let handler = match route.interaction {
            "capabilities" => {
                let statement = Arc::clone(&statement);
                on(filter, move || {
                    let statement = Arc::clone(&statement);
                    async move { body(StatusCode::OK, statement.as_ref(), FHIR_JSON) }
                })
            }
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
// TODO(#66): serve ITI-83 and ITI-104, recording audit::records::pix_query and feed.
// TODO(#70): serve ITI-78 and ITI-119, recording audit::records::pdq_search.
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
