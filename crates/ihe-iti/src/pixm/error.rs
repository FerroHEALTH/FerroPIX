// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why an ITI-83 exchange has no cross-reference to give.
//!
//! No error here carries a request URL, an identifier value, or text the PIX
//! Manager wrote (an `OperationOutcome`'s `diagnostics` or `details`, a JSON
//! parse snippet, a parameter name): the request URL holds the source
//! identifier, and the Manager's text may quote it.

use std::fmt;

use http::StatusCode;

/// An argument the client refuses before anything is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidInput {
    /// An assigning authority or a target system is not an absolute URI.
    #[error("an assigning authority is not an absolute URI")]
    System,
    /// The source identifier value is empty.
    #[error("the source identifier value is empty")]
    EmptyValue,
    /// The PIX Manager's base URL is not an `http` or `https` URL without a
    /// query or a fragment.
    #[error("the PIX Manager base URL is not an http(s) URL without a query or a fragment")]
    Base,
}

/// Why an ITI-83 exchange did not end in a cross-reference.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PixmError {
    /// The PIX Manager does not recognise the source identifier's assigning
    /// authority: a `400` with a `code-invalid` issue (§2:3.83.4.2.2.3).
    #[error("the PIX Manager does not recognise the source identifier's assigning authority")]
    SourceDomainNotRecognized,
    /// The PIX Manager does not recognise one or more target systems: a `403`
    /// with a `code-invalid` issue (§2:3.83.4.2.2.4).
    #[error("the PIX Manager does not recognise a target system")]
    TargetDomainNotRecognized,
    /// The PIX Manager answered a status the profile gives no meaning to for
    /// this request, with the issue types its `OperationOutcome` carried, if it
    /// sent one (§2:3.83.4.2.2).
    #[error("the PIX Manager answered {status}")]
    Rejected {
        /// The HTTP status.
        status: StatusCode,
        /// The issue types of the `OperationOutcome`, in order.
        issues: Vec<IssueType>,
    },
    /// No answer arrived within the timeout.
    #[error("the PIX Manager did not answer within the timeout")]
    Timeout,
    /// The request could not be sent or the answer could not be read.
    #[error("the PIX Manager could not be reached")]
    Transport(#[source] reqwest::Error),
    /// The answer does not hold to ITI-83.
    #[error("the PIX Manager's answer does not hold to ITI-83")]
    Malformed(#[from] Malformation),
}

/// How an answer departs from ITI-83 (§2:3.83.4.2.2.1, the
/// `$ihe-pix` `OperationDefinition`).
///
/// A parameter is named by its position in the answer's `parameter` list,
/// never by its name or its value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Malformation {
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
    /// The answer is a resource ITI-83 does not answer with.
    #[error("the answer is a resource ITI-83 does not answer with")]
    UnexpectedResource,
    /// The resource does not decode as FHIR R4.
    #[error("the resource does not decode as FHIR R4: {kind}")]
    Decode {
        /// Why it was refused.
        kind: fhir_types::codec::DecodeErrorKind,
    },
    /// A Bundle answer holds entries, which the profile's post-merge answer
    /// does not (§2:3.83.4.2.2.5).
    #[error("the Bundle answer holds entries")]
    BundleWithEntries,
    /// A parameter is neither `targetIdentifier` nor `targetId`.
    #[error("parameter {index} is not an output of $ihe-pix")]
    UnexpectedParameter {
        /// The parameter's position.
        index: usize,
    },
    /// A parameter carries a `resource` or nested `part`s.
    #[error("parameter {index} carries a resource or parts")]
    UnexpectedShape {
        /// The parameter's position.
        index: usize,
    },
    /// A `targetIdentifier` is not a `valueIdentifier`.
    #[error("parameter {index} is a targetIdentifier without a valueIdentifier")]
    NotAnIdentifier {
        /// The parameter's position.
        index: usize,
    },
    /// A `targetIdentifier` has no assigning authority (ITI TF-2 Appendix E.3).
    #[error("parameter {index} is an identifier without an assigning authority")]
    NoAssigningAuthority {
        /// The parameter's position.
        index: usize,
    },
    /// A `targetIdentifier` has no value.
    #[error("parameter {index} is an identifier without a value")]
    NoIdentifierValue {
        /// The parameter's position.
        index: usize,
    },
    /// A `targetIdentifier` belongs to a domain the request did not ask about
    /// (§2:3.83.4.1.2.2).
    #[error("parameter {index} is an identifier from a domain the request did not ask about")]
    UnaskedDomain {
        /// The parameter's position.
        index: usize,
    },
    /// A `targetIdentifier` is the source identifier itself, which "shall not
    /// be included in the returned Response" (§2:3.83.4.2.2.1).
    #[error("parameter {index} returns the source identifier")]
    SourceEchoed {
        /// The parameter's position.
        index: usize,
    },
    /// A `targetId` is not a `valueReference` with a `reference`.
    #[error("parameter {index} is a targetId without a reference")]
    NoReference {
        /// The parameter's position.
        index: usize,
    },
}

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

/// A transport failure, with the request URL removed: it holds the source
/// identifier (§2:3.83.4.1.2.1).
pub(super) fn transport(error: reqwest::Error) -> PixmError {
    let error = error.without_url();
    if error.is_timeout() {
        PixmError::Timeout
    } else {
        PixmError::Transport(error)
    }
}

#[cfg(test)]
mod tests {
    use super::IssueType;

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
}
