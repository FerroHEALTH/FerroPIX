// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The capabilities interaction against the PIX Manager: `GET [base]/metadata`
//! answered with the server's `CapabilityStatement`
//! (<http://hl7.org/fhir/R4/http.html#capabilities>).
//!
//! A consumer reads it to learn that the configured base is a FHIR server that
//! answers, before it asks ITI-83. The request names no patient, so its URL
//! and its answer hold no identifier.

use ferrohealth_smart::client::outbound::SendError;
use fhir_types::codec::{Json, Path};
use fhir_types::r4::capability_statement::CapabilityStatement;
use http::StatusCode;

use crate::exchange::Unanswered;
use crate::outbound::Unauthenticated;
use crate::outcome::{self, IssueType, Unreadable};

/// The longest answer the client reads. A `CapabilityStatement` lists every
/// resource and operation a server supports, which runs to a few hundred
/// kilobytes on a full FHIR server.
pub(super) const LIMIT: usize = 4 << 20;

/// The interaction under the FHIR base.
pub(super) const INTERACTION: &str = "metadata";

/// Why the capabilities interaction gave no `CapabilityStatement`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CapabilitiesError {
    /// The server answered a status other than `200`, with the issue types of
    /// its `OperationOutcome`, if it sent one.
    #[error("the PIX Manager answered {status} to the capabilities interaction")]
    Refused {
        /// The HTTP status.
        status: StatusCode,
        /// The issue types of the `OperationOutcome`, in order.
        issues: Vec<IssueType>,
    },
    /// The `200` answer is no FHIR R4 `CapabilityStatement`, so the base is no
    /// FHIR server this client can ask.
    #[error("the PIX Manager's capabilities answer is not a FHIR R4 CapabilityStatement")]
    NotAFhirServer(#[source] NotACapabilityStatement),
    /// No answer arrived within the timeout.
    #[error("the PIX Manager did not answer within the timeout")]
    Timeout,
    /// The request could not be sent or the answer could not be read.
    #[error("the PIX Manager could not be reached")]
    Transport(#[source] reqwest::Error),
    /// No token could be obtained for the request, or a refused one renewed
    /// (IUA ITI-72 §3.72.4.3).
    #[error("the request to the PIX Manager could not be authenticated")]
    Unauthenticated(#[source] SendError),
}

/// How a `200` answer fails to be a `CapabilityStatement`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NotACapabilityStatement {
    /// The answer is not a FHIR JSON resource, or does not decode as R4.
    #[error(transparent)]
    Unreadable(#[from] Unreadable),
    /// The answer is a resource of another type.
    #[error("the answer is a resource of another type")]
    OtherResource,
}

impl Unanswered for CapabilitiesError {
    fn timeout() -> Self {
        Self::Timeout
    }

    fn transport(source: reqwest::Error) -> Self {
        Self::Transport(source)
    }
}

impl Unauthenticated for CapabilitiesError {
    fn unauthenticated(source: SendError) -> Self {
        Self::Unauthenticated(source)
    }
}

/// The refusal of an answer longer than [`LIMIT`], whatever its status.
pub(super) fn too_large(_status: StatusCode) -> CapabilitiesError {
    CapabilitiesError::NotAFhirServer(Unreadable::TooLarge { limit: LIMIT }.into())
}

/// What the answer with `status`, media type `media` and `body` says.
pub(super) fn read(
    status: StatusCode,
    media: Option<&str>,
    body: &[u8],
) -> Result<CapabilityStatement, CapabilitiesError> {
    if status != StatusCode::OK {
        return Err(CapabilitiesError::Refused {
            status,
            issues: outcome::issues(media, body),
        });
    }
    let not_one = CapabilitiesError::NotAFhirServer;
    let received = outcome::resource(media, body).map_err(|e| not_one(e.into()))?;
    if received.resource_type != "CapabilityStatement" {
        return Err(not_one(NotACapabilityStatement::OtherResource));
    }
    CapabilityStatement::from_json(&received.object, &mut Path::root("CapabilityStatement"))
        .map_err(|e| not_one(Unreadable::from(e).into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FHIR_JSON: Option<&str> = Some("application/fhir+json");

    const STATEMENT: &str = r#"{
        "resourceType": "CapabilityStatement",
        "status": "active",
        "date": "2026-11-08",
        "kind": "instance",
        "fhirVersion": "4.0.1",
        "format": ["application/fhir+json"],
        "implementation": {"description": "a PIX Manager"}
    }"#;

    #[test]
    fn a_capability_statement_is_read() {
        let statement = read(StatusCode::OK, FHIR_JSON, STATEMENT.as_bytes()).expect("read");
        assert_eq!(statement.fhir_version.value.as_deref(), Some("4.0.1"));
    }

    #[test]
    fn another_resource_is_no_fhir_server() {
        let body = br#"{"resourceType": "Patient"}"#;
        assert!(matches!(
            read(StatusCode::OK, FHIR_JSON, body),
            Err(CapabilitiesError::NotAFhirServer(
                NotACapabilityStatement::OtherResource
            ))
        ));
    }

    #[test]
    fn a_body_that_is_no_fhir_json_is_no_fhir_server() {
        assert!(matches!(
            read(StatusCode::OK, Some("text/html"), b"<html></html>"),
            Err(CapabilitiesError::NotAFhirServer(
                NotACapabilityStatement::Unreadable(Unreadable::NotFhirJson)
            ))
        ));
        assert!(matches!(
            read(StatusCode::OK, FHIR_JSON, b"{\"resourceType\": "),
            Err(CapabilitiesError::NotAFhirServer(
                NotACapabilityStatement::Unreadable(Unreadable::NotJson { .. })
            ))
        ));
    }

    #[test]
    fn a_statement_that_does_not_decode_is_no_fhir_server() {
        let body = br#"{"resourceType": "CapabilityStatement", "unknown": true}"#;
        assert!(matches!(
            read(StatusCode::OK, FHIR_JSON, body),
            Err(CapabilitiesError::NotAFhirServer(
                NotACapabilityStatement::Unreadable(Unreadable::Decode { .. })
            ))
        ));
    }

    #[test]
    fn a_refusal_keeps_its_status_and_issue_types() {
        let body = br#"{"resourceType": "OperationOutcome",
            "issue": [{"severity": "error", "code": "forbidden"}]}"#;
        match read(StatusCode::FORBIDDEN, FHIR_JSON, body) {
            Err(CapabilitiesError::Refused { status, issues }) => {
                assert_eq!(status, StatusCode::FORBIDDEN);
                assert_eq!(issues, vec![IssueType::Forbidden]);
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(matches!(
            read(StatusCode::SERVICE_UNAVAILABLE, None, b""),
            Err(CapabilitiesError::Refused { issues, .. }) if issues.is_empty()
        ));
    }
}
