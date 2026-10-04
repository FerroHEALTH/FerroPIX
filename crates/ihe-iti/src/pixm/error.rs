// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why an ITI-83 exchange has no cross-reference to give.
//!
//! No error here carries a request URL or body, an identifier value, or text
//! the PIX Manager wrote (an `OperationOutcome`'s `diagnostics` or `details`, a
//! JSON parse snippet, a parameter name): the URL of a `GET` and the body of a
//! `POST` hold the source identifier, and the Manager's text may quote it.

use http::StatusCode;

use crate::outcome::IssueType;

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
    /// The `Parameters` body of a `POST` could not be written, so nothing was
    /// sent.
    #[error("the request to the PIX Manager could not be written")]
    Unwritable(#[source] serde_json::Error),
    /// The audit recorder could not accept the exchange's audit record, so
    /// its answer is not used (feature `balp`, §2:3.83.5.1.1).
    #[cfg(feature = "balp")]
    #[error("the ITI-83 audit record could not be recorded")]
    Audit(#[source] crate::balp::AuditError),
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
