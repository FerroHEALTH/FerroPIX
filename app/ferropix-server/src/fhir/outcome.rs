// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FHIR R4 `OperationOutcome` a refusal answers with.
//!
//! An issue carries a fixed diagnostics text and never a value from the
//! request: no token, no subject and no identifier reaches it. The issue
//! codes are those of R4 `IssueType`
//! (<https://hl7.org/fhir/R4/valueset-issue-type.html>).

use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use ferrohealth_server::panic::PanicBody;
use ferrohealth_server::refusal::RefusalBody;
use fhir_types::r4::operation_outcome::{OperationOutcome, OperationOutcomeIssue};
use http::{HeaderValue, StatusCode, header};

/// The media type of a FHIR JSON body (FHIR R4 `http.html`, §3.1.0.1.10).
pub const FHIR_JSON: &str = "application/fhir+json";

/// The `IssueType` codes FerroPIX answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueType {
    /// `security`: an authentication or authorization problem.
    Security,
    /// `forbidden`: the caller may not do this.
    Forbidden,
    /// `transient`: a problem that may pass, such as an upstream that could
    /// not be read.
    Transient,
    /// `not-supported`: the server does not serve the interaction.
    NotSupported,
    /// `invalid`: the request is not well formed.
    Invalid,
    /// `not-found`: what the request names does not exist.
    NotFound,
    /// `too-long`: the request body is larger than the server reads.
    TooLong,
    /// `exception`: the server could not answer.
    Exception,
    /// `conflict`: the request conflicts with what the server holds.
    Conflict,
    /// `timeout`: the request took longer than the server allows.
    Timeout,
    /// `throttled`: the server refused the request over its load.
    Throttled,
}

impl IssueType {
    /// Returns the code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Security => "security",
            Self::Forbidden => "forbidden",
            Self::Transient => "transient",
            Self::NotSupported => "not-supported",
            Self::Invalid => "invalid",
            Self::NotFound => "not-found",
            Self::TooLong => "too-long",
            Self::Exception => "exception",
            Self::Conflict => "conflict",
            Self::Timeout => "timeout",
            Self::Throttled => "throttled",
        }
    }
}

/// Returns the `OperationOutcome` of one `error` issue of `code` with
/// `diagnostics`.
#[must_use]
pub fn document(code: IssueType, diagnostics: &'static str) -> OperationOutcome {
    OperationOutcome {
        issue: vec![OperationOutcomeIssue {
            severity: "error".into(),
            code: code.code().into(),
            diagnostics: Some(diagnostics.into()),
            ..OperationOutcomeIssue::default()
        }],
        ..OperationOutcome::default()
    }
}

/// Returns `resource` as a body of `status` and `media_type` that no cache
/// keeps, or a `500` when it cannot be written.
pub fn body<T: serde::Serialize>(
    status: StatusCode,
    resource: &T,
    media_type: &'static str,
) -> Response {
    let Ok(text) = serde_json::to_string(resource) else {
        tracing::error!("a FerroPIX response body could not be written");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = (status, text).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(media_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Returns the response of `status` carrying the `OperationOutcome` of one
/// `error` issue, with the `WWW-Authenticate` `challenge` when one is given.
#[must_use]
pub fn respond(
    status: StatusCode,
    code: IssueType,
    diagnostics: &'static str,
    challenge: Option<&str>,
) -> Response {
    let mut response = body(status, &document(code, diagnostics), FHIR_JSON);
    if let Some(value) = challenge.and_then(|challenge| HeaderValue::from_str(challenge).ok()) {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

/// The body of a request whose handler panicked.
///
/// It is an `OperationOutcome` of one `exception` issue naming the server's
/// request id, the id the request log and the panic line carry, and never
/// the panic message, since a handler may have panicked over an identifier.
#[must_use]
pub fn panic_body() -> PanicBody {
    Arc::new(|_uri, ids| {
        let diagnostics = format!(
            "the server could not complete the request (request id {})",
            ids.server
        );
        let outcome = OperationOutcome {
            issue: vec![OperationOutcomeIssue {
                severity: "error".into(),
                code: IssueType::Exception.code().into(),
                diagnostics: Some(diagnostics.into()),
                ..OperationOutcomeIssue::default()
            }],
            ..OperationOutcome::default()
        };
        body(StatusCode::INTERNAL_SERVER_ERROR, &outcome, FHIR_JSON)
    })
}

/// The body of a refusal the stack of `ferrohealth-server` writes itself.
///
/// A `408`, `413`, `429` or `503` is an `OperationOutcome` of the issue type
/// [`refusal_type`] names, with a fixed diagnostics text, so every refusal
/// of the server has one shape (FHIR R4 `http.html`, §3.1.0.4.2).
#[must_use]
pub fn refusal_body() -> RefusalBody {
    Arc::new(|_uri, _ids, status, _reason| {
        let (code, diagnostics) = refusal_type(status);
        body(status, &document(code, diagnostics), FHIR_JSON)
    })
}

/// The issue type and the diagnostics of a stack refusal of `status`
/// (<https://hl7.org/fhir/R4/valueset-issue-type.html>).
#[must_use]
pub const fn refusal_type(status: StatusCode) -> (IssueType, &'static str) {
    match status {
        StatusCode::REQUEST_TIMEOUT => (
            IssueType::Timeout,
            "the request took longer than this server allows",
        ),
        StatusCode::PAYLOAD_TOO_LARGE => (
            IssueType::TooLong,
            "the request body is larger than this server accepts",
        ),
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE => (
            IssueType::Throttled,
            "the server is at its limit of requests; send the request again later",
        ),
        _ => (
            IssueType::Exception,
            "the server could not complete the request",
        ),
    }
}

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use super::{IssueType, document, refusal_type, respond};

    #[test]
    fn each_refusal_of_the_stack_has_its_issue_type() {
        for (status, code) in [
            (StatusCode::REQUEST_TIMEOUT, IssueType::Timeout),
            (StatusCode::PAYLOAD_TOO_LARGE, IssueType::TooLong),
            (StatusCode::TOO_MANY_REQUESTS, IssueType::Throttled),
            (StatusCode::SERVICE_UNAVAILABLE, IssueType::Throttled),
            (StatusCode::BAD_GATEWAY, IssueType::Exception),
        ] {
            assert_eq!(code, refusal_type(status).0, "{status}");
        }
    }

    #[test]
    #[expect(
        clippy::disallowed_types,
        reason = "the test reads the outcome in its wire form"
    )]
    fn an_outcome_is_one_error_issue_with_its_code_and_diagnostics() {
        let outcome: serde_json::Value =
            serde_json::to_value(document(IssueType::Forbidden, "no")).expect("it serializes");
        assert_eq!("OperationOutcome", outcome["resourceType"]);
        assert_eq!("error", outcome["issue"][0]["severity"]);
        assert_eq!("forbidden", outcome["issue"][0]["code"]);
        assert_eq!("no", outcome["issue"][0]["diagnostics"]);
        let response = respond(
            StatusCode::UNAUTHORIZED,
            IssueType::Security,
            "no",
            Some("Bearer"),
        );
        assert_eq!(StatusCode::UNAUTHORIZED, response.status());
        assert_eq!(
            Some("Bearer"),
            response
                .headers()
                .get(http::header::WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok())
        );
    }
}
