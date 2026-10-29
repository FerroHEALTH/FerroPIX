// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The RFC 9457 problem details every error body outside the FHIR and IHE
//! routes answers with (<https://www.rfc-editor.org/rfc/rfc9457>).
//!
//! FerroPIX's problem types are defined once, in [`types`], under
//! [`BASE`], the book page that documents each one (RFC 9457 §4): the
//! `type` of [`Kind::NotFound`] is `{BASE}not-found`. A refusal whose
//! status says all there is to say is `about:blank` ([`blank`]). A detail
//! is a `&'static str`, so no problem can quote the request, an identifier
//! or a pseudonym (§5). The FHIR face and the XCPD gateway keep the
//! `OperationOutcome` and the SOAP fault their specifications prescribe
//! (FHIR R4 `http.html` §3.1.0.4.2): [`is_fhir_or_ihe`] tells them apart.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

use axum::response::{IntoResponse as _, Response};
use ferrohealth_server::panic::PanicBody;
use ferrohealth_server::problem::{Problem, ProblemType, ProblemTypes};
use ferrohealth_server::refusal::RefusalBody;
use ferrohealth_server::request_id::RequestIds;
use http::{HeaderValue, StatusCode, Uri, header};

use crate::fhir::outcome;

/// The base URI of every FerroPIX problem type: the book page that
/// documents them, one heading per slug, as the console reads it.
pub const BASE: &str = ferropix_operator_api::problem::BASE;

/// A FerroPIX problem type, each defined once in [`types`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// `400`: the body or the query is not what the route reads.
    InvalidRequest,
    /// `400`: a change or a disclosure states no reason, or one too long.
    ReasonRequired,
    /// `401`: the request carries no valid credential.
    Unauthenticated,
    /// `403`: the credential does not permit the request.
    NotPermitted,
    /// `404`: what the request names does not exist.
    NotFound,
    /// `409`: what the request was decided on changed since it was read.
    Stale,
    /// `409`: a later merge of the survivor has to be reverted first.
    LaterMerge,
    /// `409`: the domain is retired.
    Retired,
    /// `409`: another domain holds the system or the namespace.
    AlreadyHeld,
    /// `422`: the identifier domain is unknown or retired.
    UnknownDomain,
    /// `422`: no issuer and no Basic user admits the Source.
    SourceNotAdmitted,
    /// `503`: the cross-reference store could not be read.
    StoreUnavailable,
    /// `503`: the audit record of the request could not be stored.
    AuditUnavailable,
    /// `503`: the access token could not be checked.
    AuthorizationUnavailable,
}

impl Kind {
    /// Every kind, in the order the book lists them.
    pub const ALL: [Self; 14] = [
        Self::InvalidRequest,
        Self::ReasonRequired,
        Self::Unauthenticated,
        Self::NotPermitted,
        Self::NotFound,
        Self::Stale,
        Self::LaterMerge,
        Self::Retired,
        Self::AlreadyHeld,
        Self::UnknownDomain,
        Self::SourceNotAdmitted,
        Self::StoreUnavailable,
        Self::AuditUnavailable,
        Self::AuthorizationUnavailable,
    ];

    /// The slug, the fragment of the type under [`BASE`], its title and
    /// its status.
    #[must_use]
    pub const fn definition(self) -> (&'static str, &'static str, StatusCode) {
        match self {
            Self::InvalidRequest => (
                "invalid-request",
                "The request is not what this route reads",
                StatusCode::BAD_REQUEST,
            ),
            Self::ReasonRequired => (
                "reason-required",
                "The request states no reason, or one longer than 500 characters",
                StatusCode::BAD_REQUEST,
            ),
            Self::Unauthenticated => (
                "unauthenticated",
                "The request carries no valid access token",
                StatusCode::UNAUTHORIZED,
            ),
            Self::NotPermitted => (
                "not-permitted",
                "The access token does not permit this request",
                StatusCode::FORBIDDEN,
            ),
            Self::NotFound => (
                "not-found",
                "What the request names does not exist",
                StatusCode::NOT_FOUND,
            ),
            Self::Stale => (
                "stale",
                "What the request was decided on changed since it was read",
                StatusCode::CONFLICT,
            ),
            Self::LaterMerge => (
                "later-merge",
                "A later merge of the survivor has to be reverted first",
                StatusCode::CONFLICT,
            ),
            Self::Retired => ("retired", "The domain is retired", StatusCode::CONFLICT),
            Self::AlreadyHeld => (
                "already-held",
                "Another domain holds this system or namespace",
                StatusCode::CONFLICT,
            ),
            Self::UnknownDomain => (
                "unknown-domain",
                "The identifier domain is unknown or retired",
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            Self::SourceNotAdmitted => (
                "source-not-admitted",
                "No issuer and no Basic user admits this Source",
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            Self::StoreUnavailable => (
                "store-unavailable",
                "The cross-reference store could not be read",
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            Self::AuditUnavailable => (
                "audit-unavailable",
                "The audit record of the request could not be stored",
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            Self::AuthorizationUnavailable => (
                "authorization-unavailable",
                "The access token could not be checked",
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        }
    }

    /// The problem type of this kind, as [`types`] defines it.
    ///
    /// # Panics
    ///
    /// Never: the registry defines every kind, which a unit test holds.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "registry() defines every Kind::ALL entry, which every_kind_is_defined_once checks"
    )]
    pub fn problem_type(self) -> &'static ProblemType {
        let (types, by_kind) = &*REGISTRY;
        by_kind
            .get(&self)
            .and_then(|slug| types.get(slug))
            .expect("every kind should be defined in the registry")
    }

    /// A problem of this kind with `detail`, a fixed sentence that quotes
    /// nothing from the request.
    #[must_use]
    pub fn problem(self, detail: &'static str) -> Problem {
        self.problem_type().problem().with_detail(detail)
    }
}

/// The registry and the slug of each kind.
static REGISTRY: LazyLock<(ProblemTypes, BTreeMap<Kind, &'static str>)> = LazyLock::new(registry);

/// Builds the registry of every [`Kind`].
#[expect(
    clippy::expect_used,
    reason = "BASE and every definition are constants that every_kind_is_defined_once builds"
)]
fn registry() -> (ProblemTypes, BTreeMap<Kind, &'static str>) {
    let mut types = ProblemTypes::new(BASE).expect("the problem base should be a legal base");
    let mut by_kind = BTreeMap::new();
    for kind in Kind::ALL {
        let (slug, title, status) = kind.definition();
        types
            .define(slug, title, status)
            .expect("every problem type should be defined once with a legal slug");
        by_kind.insert(kind, slug);
    }
    (types, by_kind)
}

/// FerroPIX's problem types, in slug order.
///
/// # Panics
///
/// Never: [`BASE`] and every definition are constants a unit test builds.
#[must_use]
pub fn types() -> &'static ProblemTypes {
    &REGISTRY.0
}

/// Returns `problem` as its response, never kept by a cache, with the
/// `WWW-Authenticate` `challenge` when one is given.
#[must_use]
pub fn respond(problem: Problem, challenge: Option<&str>) -> Response {
    let mut response = problem.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(value) = challenge.and_then(|challenge| HeaderValue::from_str(challenge).ok()) {
        headers.insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

/// Returns the response of a problem of `kind` with `detail`.
#[must_use]
pub fn refuse(kind: Kind, detail: &'static str) -> Response {
    respond(kind.problem(detail), None)
}

/// Returns the response of an `about:blank` problem of `status`, whose
/// status says what there is to say, with `detail` (RFC 9457 §4.2.1).
#[must_use]
pub fn blank(status: StatusCode, detail: &'static str) -> Response {
    respond(Problem::new(status).with_detail(detail), None)
}

/// Whether `path` is under the FHIR face or the XCPD gateway of the server
/// mounted at `base`, whose error bodies their specifications prescribe.
#[must_use]
pub fn is_fhir_or_ihe(base: &str, path: &str) -> bool {
    let Some(relative) = path.strip_prefix(base) else {
        return false;
    };
    [crate::auth::table::FHIR, crate::xcpd::PATH]
        .iter()
        .any(|prefix| {
            relative
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        })
}

/// The body of a request whose handler panicked: the FHIR face's
/// `OperationOutcome` under it and the XCPD gateway, a problem elsewhere.
#[must_use]
pub fn panic_body(base: &str) -> PanicBody {
    let base = base.to_owned();
    let (fhir, problem) = (
        outcome::panic_body(),
        ferrohealth_server::problem::panic_body(),
    );
    Arc::new(move |uri: &Uri, ids: RequestIds<'_>| {
        if is_fhir_or_ihe(&base, uri.path()) {
            fhir(uri, ids)
        } else {
            problem(uri, ids)
        }
    })
}

/// The body of a refusal the stack writes itself: the FHIR face's
/// `OperationOutcome` under it and the XCPD gateway, an `about:blank`
/// problem elsewhere.
#[must_use]
pub fn refusal_body(base: &str) -> RefusalBody {
    let base = base.to_owned();
    let (fhir, problem) = (
        outcome::refusal_body(),
        ferrohealth_server::problem::refusal_body(),
    );
    Arc::new(
        move |uri: &Uri, ids: RequestIds<'_>, status: StatusCode, reason: &str| {
            if is_fhir_or_ihe(&base, uri.path()) {
                fhir(uri, ids, status, reason)
            } else {
                problem(uri, ids, status, reason)
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use http::StatusCode;

    use super::{BASE, Kind, is_fhir_or_ihe, types};

    #[test]
    fn every_kind_is_defined_once_under_the_book_page() {
        let mut slugs = BTreeSet::new();
        for kind in Kind::ALL {
            let defined = kind.problem_type();
            let (slug, title, status) = kind.definition();
            assert_eq!(format!("{BASE}{slug}"), defined.uri());
            assert_eq!(Some(title), defined.title());
            assert_eq!(status, defined.status());
            assert!(slugs.insert(slug), "{slug} is defined twice");
        }
        assert_eq!(Kind::ALL.len(), types().iter().count());
    }

    #[test]
    fn the_fhir_face_and_the_xcpd_gateway_keep_their_bodies() {
        for path in ["/pix/fhir", "/pix/fhir/Patient", "/pix/xcpd"] {
            assert!(is_fhir_or_ihe("/pix", path), "{path}");
        }
        for path in [
            "/pix/operator/overview",
            "/pix/health/readiness",
            "/pix/metrics",
            "/pix/fhirish",
            "/fhir",
        ] {
            assert!(!is_fhir_or_ihe("/pix", path), "{path}");
        }
        assert!(is_fhir_or_ihe("", "/fhir/metadata"));
    }

    #[test]
    fn a_problem_answers_its_status_as_problem_json_no_cache_keeps() {
        let response = super::refuse(Kind::Stale, "reload it");
        assert_eq!(StatusCode::CONFLICT, response.status());
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        assert_eq!(
            Some("application/problem+json".to_owned()),
            header(http::header::CONTENT_TYPE)
        );
        assert_eq!(
            Some("no-store".to_owned()),
            header(http::header::CACHE_CONTROL)
        );
    }
}
