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

use fhir_types::codec::{Json, Path, Value};
use fhir_types::r4::operation_outcome::OperationOutcome;

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

/// Whether `media` is FHIR JSON (ITI TF-2 Appendix Z.6), or plain JSON, which
/// FHIR R4 accepts for the same content (<http://hl7.org/fhir/R4/http.html#mime-type>).
pub(crate) fn fhir_json(media: Option<&str>) -> bool {
    media
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|essence| {
            essence.eq_ignore_ascii_case("application/fhir+json")
                || essence.eq_ignore_ascii_case("application/json")
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

/// The issue types of a failure answer's `OperationOutcome`, or none when the
/// body is not one.
pub(crate) fn issues(media: Option<&str>, body: &[u8]) -> Vec<IssueType> {
    if !fhir_json(media) {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    if object.get("resourceType").and_then(Value::as_str) != Some("OperationOutcome") {
        return Vec::new();
    }
    // NOTE: FHIR R4 http.html, a failure body that is no `OperationOutcome` is a
    // status alone; the status decides, and an undecodable outcome adds nothing.
    let Ok(outcome) = OperationOutcome::from_json(object, &mut Path::root("OperationOutcome"))
    else {
        return Vec::new();
    };
    of(&outcome)
}

#[cfg(test)]
mod tests {
    use super::{IssueType, fhir_json, issues};

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
    fn fhir_json_and_plain_json_are_accepted_with_parameters() {
        assert!(fhir_json(Some("application/fhir+json; fhirVersion=4.0")));
        assert!(fhir_json(Some("Application/JSON")));
        assert!(!fhir_json(Some("text/html")));
        assert!(!fhir_json(None));
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
