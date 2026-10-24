// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The guard in front of the FHIR face and the operator API.
//!
//! The `ferrohealth-smart` layer reads [`crate::auth::table`]; the check
//! behind it holds what a scope reaches and the domain a Source writes to;
//! every refusal answers with an `OperationOutcome`.
//!
//! A request without a valid credential answers `401` and one whose
//! credential grants no permission for the route `403` (RFC 6750 §3.1),
//! each with a `security` issue. A Source writing to a domain it is not the
//! Source of answers `403` with a `forbidden` issue (PIXm 3.1.0 §2:3.104.5).
//! No refusal names a claim, a scope, the token or a value of the request.

use std::sync::Arc;

use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use ferrohealth_smart::guard::{AuthLayer, Classify, Render, Requirement, Target};
use ferrohealth_smart::refusal::Refusal;
use ferrohealth_smart::resource::{Access, Compartment};
use http::request::Parts;
use http::{Extensions, Method, StatusCode, Uri};

use crate::auth::Smart;
use crate::auth::source::{Verdict, domains_of};
use crate::auth::table::{self, Need, Route};
use crate::fhir::outcome::{IssueType, respond};

/// The diagnostics of a request that carries no valid credential.
pub const UNAUTHENTICATED: &str = "the request carries no valid access token for this server";

/// The diagnostics of a credential that does not permit the request.
pub const NOT_PERMITTED: &str = "the access token does not permit this request";

/// The diagnostics of a Source writing to a domain it is not the Source of.
pub const NOT_THE_SOURCE: &str =
    "the caller is not the Patient Identity Source of the identifier domain this request changes";

/// Returns the route of `method` on the matched route of `extensions`
/// under `{base}` `base`.
fn route_of(base: &str, method: &Method, extensions: &Extensions) -> Option<&'static Route> {
    let matched = extensions.get::<MatchedPath>()?.as_str();
    let template = matched.strip_prefix(base).unwrap_or(matched);
    table::find(method, template)
}

/// How the layer reads a request: the row of the permission table its
/// matched route is.
#[derive(Debug, Clone)]
pub struct Classifier {
    /// The base path every route is mounted under.
    base: String,
    /// The scope of a data steward.
    steward: String,
    /// The scope of an administrator.
    admin: String,
}

impl Classifier {
    /// Returns the classifier of routes under `base`, with the product
    /// scopes of `smart`.
    #[must_use]
    pub fn new(base: &str, smart: &Smart) -> Self {
        Self {
            base: base.to_owned(),
            steward: smart.steward_scope().to_owned(),
            admin: smart.admin_scope().to_owned(),
        }
    }
}

impl Classify for Classifier {
    fn classify(&self, method: &Method, uri: &Uri, extensions: &Extensions) -> Option<Target> {
        let relative = uri.path().strip_prefix(&self.base)?;
        if !table::is_guarded(relative) {
            return None;
        }
        let Some(route) = route_of(&self.base, method, extensions) else {
            // NOTE: no specification governs this: our own design; a path no row
            // names needs a credential, so an anonymous probe learns nothing.
            return Some(Target::new("unclassified", Requirement::Caller));
        };
        let target = Target::new(
            route.interaction,
            route.need.requirement(&self.steward, &self.admin),
        )
        .route(route.path);
        Some(match route.need {
            Need::Fhir { resource_type, .. } => target.target(resource_type),
            Need::Public | Need::Steward | Need::Admin | Need::StewardOrAdmin => target,
        })
    }
}

/// The `OperationOutcome` every refusal of the layer answers with.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    /// The realm every challenge names.
    realm: Option<String>,
}

impl Outcome {
    /// Returns the response of `refusal`.
    fn respond(&self, refusal: &Refusal) -> Response {
        let (code, diagnostics) = match refusal {
            Refusal::Unavailable => (
                IssueType::Transient,
                "the access token could not be checked: the authorization server could not be read",
            ),
            Refusal::Forbidden | Refusal::Unclassified => (IssueType::Security, NOT_PERMITTED),
            _ => (IssueType::Security, UNAUTHENTICATED),
        };
        respond(
            refusal.status(),
            code,
            diagnostics,
            refusal.challenge(self.realm.as_deref()).as_deref(),
        )
    }
}

impl Render for Outcome {
    fn render(&self, refusal: &Refusal, _parts: &Parts) -> Response {
        self.respond(refusal)
    }
}

/// Returns the layer that guards the FHIR face and the operator API under
/// `base` with the gate of `smart`.
#[must_use]
pub fn layer(base: &str, smart: &Smart) -> AuthLayer<Classifier> {
    let gate = smart.gate().clone();
    let render = Outcome {
        realm: gate.realm().map(str::to_owned),
    };
    AuthLayer::new(gate, Classifier::new(base, smart)).with_render(render)
}

/// What the check behind the gate reads.
#[derive(Debug, Clone)]
pub struct Confine {
    /// The base path every route is mounted under.
    base: String,
    /// The resource server, with the Sources of every domain.
    smart: Arc<Smart>,
}

impl Confine {
    /// Returns the check of routes under `base` against `smart`.
    #[must_use]
    pub fn new(base: &str, smart: Arc<Smart>) -> Self {
        Self {
            base: base.to_owned(),
            smart,
        }
    }
}

/// The check behind the gate: a FHIR route needs a grant that reaches every
/// resource of its type, and a Source route needs the caller to be the
/// Source of every domain the request names.
///
/// A `patient/` scope reaches one patient's compartment, which a Master
/// Patient Index has no use for, so it is refused as a missing permission
/// (no specification governs this: our own design).
pub async fn confine(
    State(confine): State<Arc<Confine>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(access) = request.extensions().get::<Access>().cloned() else {
        return next.run(request).await;
    };
    let Some(route) = route_of(&confine.base, request.method(), request.extensions()) else {
        return next.run(request).await;
    };
    let Need::Fhir {
        permissions,
        resource_type,
        transaction,
        source,
    } = route.need
    else {
        return next.run(request).await;
    };
    let by_transaction = transaction.is_some_and(|scope| access.has_scope(scope));
    let system_wide = permissions.iter().any(|permission| {
        matches!(
            access.reach(*permission, resource_type),
            Some(Compartment::Any)
        )
    });
    if !(by_transaction || system_wide) {
        return Outcome {
            realm: confine.smart.gate().realm().map(str::to_owned),
        }
        .respond(&Refusal::Forbidden);
    }
    if source {
        let sources = confine.smart.sources();
        let foreign = domains_of(request.uri().query())
            .iter()
            .any(|system| sources.verdict(&access, system) == Verdict::Forbidden);
        if foreign {
            return respond(
                StatusCode::FORBIDDEN,
                IssueType::Forbidden,
                NOT_THE_SOURCE,
                None,
            );
        }
    }
    next.run(request).await
}
