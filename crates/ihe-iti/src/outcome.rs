// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What a FHIR server's failure answer carries that a client may keep: the
//! `issue-type` codes of its `OperationOutcome`, and nothing of its text.
//!
//! Every IHE transaction over FHIR answers a failure with an
//! `OperationOutcome`, whose `diagnostics` and `details` are free text that may
//! quote the request, and so a patient identifier or a demographic value. A
//! client keeps the issue codes, which are drawn from a closed value set, and
//! drops the rest.

use std::fmt;

use fhir_types::codec::{DecodeError, DecodeErrorKind, Json, Object, Path, Value};
use fhir_types::r4::operation_outcome::OperationOutcome;

use crate::rest::fhir_json;

/// The FHIR R4 `issue-type` of an `OperationOutcome` issue
/// (<http://hl7.org/fhir/R4/valueset-issue-type.html>).
///
/// A code outside the value set is [`IssueType::Unrecognized`] and its text is
/// dropped, so an answer cannot carry free text through this field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IssueType {
    /// `invalid`
    Invalid,
    /// `structure`
    Structure,
    /// `required`
    Required,
    /// `value`
    Value,
    /// `invariant`
    Invariant,
    /// `security`
    Security,
    /// `login`
    Login,
    /// `unknown`
    Unknown,
    /// `expired`
    Expired,
    /// `forbidden`
    Forbidden,
    /// `suppressed`
    Suppressed,
    /// `processing`
    Processing,
    /// `not-supported`
    NotSupported,
    /// `duplicate`
    Duplicate,
    /// `multiple-matches`
    MultipleMatches,
    /// `not-found`
    NotFound,
    /// `deleted`
    Deleted,
    /// `too-long`
    TooLong,
    /// `code-invalid`
    CodeInvalid,
    /// `extension`
    Extension,
    /// `too-costly`
    TooCostly,
    /// `business-rule`
    BusinessRule,
    /// `conflict`
    Conflict,
    /// `transient`
    Transient,
    /// `lock-error`
    LockError,
    /// `no-store`
    NoStore,
    /// `exception`
    Exception,
    /// `timeout`
    Timeout,
    /// `incomplete`
    Incomplete,
    /// `throttled`
    Throttled,
    /// `informational`
    Informational,
    /// A code outside the R4 value set, or none.
    Unrecognized,
}

impl IssueType {
    /// Returns the issue type `code` names.
    #[must_use]
    pub fn from_code(code: &str) -> Self {
        match code {
            "invalid" => Self::Invalid,
            "structure" => Self::Structure,
            "required" => Self::Required,
            "value" => Self::Value,
            "invariant" => Self::Invariant,
            "security" => Self::Security,
            "login" => Self::Login,
            "unknown" => Self::Unknown,
            "expired" => Self::Expired,
            "forbidden" => Self::Forbidden,
            "suppressed" => Self::Suppressed,
            "processing" => Self::Processing,
            "not-supported" => Self::NotSupported,
            "duplicate" => Self::Duplicate,
            "multiple-matches" => Self::MultipleMatches,
            "not-found" => Self::NotFound,
            "deleted" => Self::Deleted,
            "too-long" => Self::TooLong,
            "code-invalid" => Self::CodeInvalid,
            "extension" => Self::Extension,
            "too-costly" => Self::TooCostly,
            "business-rule" => Self::BusinessRule,
            "conflict" => Self::Conflict,
            "transient" => Self::Transient,
            "lock-error" => Self::LockError,
            "no-store" => Self::NoStore,
            "exception" => Self::Exception,
            "timeout" => Self::Timeout,
            "incomplete" => Self::Incomplete,
            "throttled" => Self::Throttled,
            "informational" => Self::Informational,
            _ => Self::Unrecognized,
        }
    }
}

impl fmt::Display for IssueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "invalid",
            Self::Structure => "structure",
            Self::Required => "required",
            Self::Value => "value",
            Self::Invariant => "invariant",
            Self::Security => "security",
            Self::Login => "login",
            Self::Unknown => "unknown",
            Self::Expired => "expired",
            Self::Forbidden => "forbidden",
            Self::Suppressed => "suppressed",
            Self::Processing => "processing",
            Self::NotSupported => "not-supported",
            Self::Duplicate => "duplicate",
            Self::MultipleMatches => "multiple-matches",
            Self::NotFound => "not-found",
            Self::Deleted => "deleted",
            Self::TooLong => "too-long",
            Self::CodeInvalid => "code-invalid",
            Self::Extension => "extension",
            Self::TooCostly => "too-costly",
            Self::BusinessRule => "business-rule",
            Self::Conflict => "conflict",
            Self::Transient => "transient",
            Self::LockError => "lock-error",
            Self::NoStore => "no-store",
            Self::Exception => "exception",
            Self::Timeout => "timeout",
            Self::Incomplete => "incomplete",
            Self::Throttled => "throttled",
            Self::Informational => "informational",
            Self::Unrecognized => "unrecognized",
        })
    }
}

/// How a client's answer fails to be a FHIR JSON resource, before its
/// profile reads it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Unreadable {
    /// The answer's media type is not FHIR JSON.
    #[error("the answer is not application/fhir+json")]
    NotFhirJson,
    /// The answer is longer than the client reads.
    #[error("the answer exceeds {limit} bytes")]
    TooLarge {
        /// The limit, in bytes.
        limit: usize,
    },
    /// The answer is not JSON.
    #[error("the answer is not JSON (line {line}, column {column})")]
    NotJson {
        /// The line of the syntax error.
        line: usize,
        /// The column of the syntax error.
        column: usize,
    },
    /// The answer is not a JSON object with a `resourceType`.
    #[error("the answer is not a FHIR resource")]
    NotAResource,
    /// The resource does not decode as FHIR R4.
    #[error("the resource does not decode as FHIR R4: {kind}")]
    Decode {
        /// Why it was refused.
        kind: DecodeErrorKind,
    },
}

impl From<DecodeError> for Unreadable {
    fn from(error: DecodeError) -> Self {
        Self::Decode { kind: error.kind }
    }
}

/// A FHIR JSON resource an answer holds, not yet decoded.
#[cfg(any(feature = "pixm", feature = "pdqm"))]
pub(crate) struct Received {
    /// Its `resourceType`.
    pub(crate) resource_type: String,
    /// Its members, `resourceType` among them.
    pub(crate) object: Object,
}

/// The resource of an answer with media type `media` and `body`.
#[cfg(any(feature = "pixm", feature = "pdqm"))]
pub(crate) fn resource(media: Option<&str>, body: &[u8]) -> Result<Received, Unreadable> {
    if !fhir_json(media) {
        return Err(Unreadable::NotFhirJson);
    }
    let value: Value = serde_json::from_slice(body).map_err(|error| Unreadable::NotJson {
        line: error.line(),
        column: error.column(),
    })?;
    let Value::Object(object) = value else {
        return Err(Unreadable::NotAResource);
    };
    let resource_type = object
        .get("resourceType")
        .and_then(Value::as_str)
        .ok_or(Unreadable::NotAResource)?
        .to_owned();
    Ok(Received {
        resource_type,
        object,
    })
}

/// The issue types of an `OperationOutcome` resource already parsed, in order.
pub(crate) fn of(outcome: &OperationOutcome) -> Vec<IssueType> {
    outcome
        .issue
        .iter()
        .map(|issue| {
            issue
                .code
                .value
                .as_deref()
                .map_or(IssueType::Unrecognized, IssueType::from_code)
        })
        .collect()
}

/// The JSON object of a failure answer's FHIR JSON body, or none when it holds
/// none: a failure body is read for its issue codes only, and the status
/// decides.
pub(crate) fn failure_object(media: Option<&str>, body: &[u8]) -> Option<Object> {
    if !fhir_json(media) {
        return None;
    }
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(object)) => Some(object),
        _ => None,
    }
}

/// The issue types of a failure answer's `OperationOutcome`, or none when the
/// body is not one.
pub(crate) fn issues(media: Option<&str>, body: &[u8]) -> Vec<IssueType> {
    let Some(object) = failure_object(media, body) else {
        return Vec::new();
    };
    if object.get("resourceType").and_then(Value::as_str) != Some("OperationOutcome") {
        return Vec::new();
    }
    // NOTE: FHIR R4 http.html, a failure body that is no `OperationOutcome` is a
    // status alone; the status decides, and an undecodable outcome adds nothing.
    let Ok(outcome) = OperationOutcome::from_json(&object, &mut Path::root("OperationOutcome"))
    else {
        return Vec::new();
    };
    of(&outcome)
}

#[cfg(test)]
mod tests {
    use super::{IssueType, issues};

    #[test]
    fn every_code_round_trips_and_free_text_is_unrecognized() {
        for code in [
            "invalid",
            "not-found",
            "code-invalid",
            "multiple-matches",
            "informational",
        ] {
            assert_eq!(
                IssueType::from_code(code).to_string(),
                code,
                "{code} is in the R4 issue-type value set"
            );
        }
        assert_eq!(
            IssueType::from_code("SENTINEL-4711"),
            IssueType::Unrecognized,
            "free text in a code field is dropped"
        );
    }

    #[test]
    fn an_outcome_yields_its_codes_and_nothing_else_does() {
        let body = br#"{"resourceType":"OperationOutcome","issue":[{"severity":"error","code":"not-found","diagnostics":"SENTINEL"}]}"#;
        assert_eq!(
            issues(Some("application/fhir+json"), body),
            [IssueType::NotFound],
            "the code, never the diagnostics"
        );
        assert!(issues(Some("text/plain"), body).is_empty(), "not FHIR JSON");
        assert!(
            issues(
                Some("application/fhir+json"),
                br#"{"resourceType":"Patient"}"#
            )
            .is_empty(),
            "not an OperationOutcome"
        );
    }
}
