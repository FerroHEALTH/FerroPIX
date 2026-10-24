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
use crate::http::request_log::{Caller, Written};

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
    let caller = Caller(caller_of(&access));
    let mut response = match check(&confine, &access, &request) {
        Checked::Refused(response) => response,
        Checked::Pass => next.run(request).await,
        Checked::Feed(systems) => {
            let mut response = next.run(request).await;
            if response.status().is_success() {
                response.extensions_mut().insert(Written(systems));
            }
            response
        }
    };
    response.extensions_mut().insert(caller);
    response
}

/// What the check behind the gate decided.
enum Checked {
    /// The request passes.
    Pass,
    /// The request passes, writing to these domains.
    Feed(Vec<String>),
    /// The request is refused with this response.
    Refused(Response),
}

/// Returns what the check behind the gate decides for `request` of `access`.
fn check(confine: &Confine, access: &Access, request: &Request) -> Checked {
    let Some(route) = route_of(&confine.base, request.method(), request.extensions()) else {
        return Checked::Pass;
    };
    let Need::Fhir {
        permissions,
        resource_type,
        transaction,
        source,
    } = route.need
    else {
        return Checked::Pass;
    };
    let by_transaction = transaction.is_some_and(|scope| access.has_scope(scope));
    let system_wide = permissions.iter().any(|permission| {
        matches!(
            access.reach(*permission, resource_type),
            Some(Compartment::Any)
        )
    });
    if !(by_transaction || system_wide) {
        return Checked::Refused(
            Outcome {
                realm: confine.smart.gate().realm().map(str::to_owned),
            }
            .respond(&Refusal::Forbidden),
        );
    }
    if !source {
        return Checked::Pass;
    }
    let sources = confine.smart.sources();
    let systems = domains_of(request.uri().query());
    if systems
        .iter()
        .any(|system| sources.verdict(access, system) == Verdict::Forbidden)
    {
        return Checked::Refused(respond(
            StatusCode::FORBIDDEN,
            IssueType::Forbidden,
            NOT_THE_SOURCE,
            None,
        ));
    }
    Checked::Feed(systems)
}

/// Returns the caller the request log names for `access`: the token's
/// `client_id`, or its subject or the Basic user-id when it names none.
fn caller_of(access: &Access) -> String {
    access.caller().map_or_else(
        || String::from("-"),
        |caller| caller.client_id().unwrap_or(caller.subject()).to_owned(),
    )
}
