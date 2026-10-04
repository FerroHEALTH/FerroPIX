// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why an ITI-78 exchange has no result set to give.
//!
//! No error here carries a demographic value, a request URL or body, a page
//! link, or text the Patient Demographics Supplier wrote (an
//! `OperationOutcome`'s `diagnostics` or `details`, a JSON parse snippet): the
//! query is directly identifying, and the Supplier's text may quote it.

use http::StatusCode;

use crate::outcome::IssueType;

/// An argument the client refuses before anything is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidInput {
    /// An identifier system is not an absolute URI.
    #[error("an identifier system is not an absolute URI")]
    System,
    /// A search value is empty.
    #[error("a search value is empty")]
    EmptyValue,
    /// An `_id` value is not a FHIR `id`: one to 64 of `A-Z`, `a-z`, `0-9`,
    /// `-` and `.`.
    #[error("an _id value is not a FHIR id")]
    Id,
    /// A `birthdate` value is not a FHIR `date`: `YYYY`, `YYYY-MM` or
    /// `YYYY-MM-DD`.
    #[error("a birthdate value is not a FHIR date")]
    Date,
    /// The Supplier's base URL is not an `http` or `https` URL without a query
    /// or a fragment.
    #[error("the Supplier base URL is not an http(s) URL without a query or a fragment")]
    Base,
}

/// Why an ITI-78 exchange did not end in a result set.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PdqmError {
    /// The Supplier does not recognise an identifier domain the query names:
    /// a `404` with a `not-found` issue (§2:3.78.4.1.3, Case 4).
    #[error("the Supplier does not recognise an identifier domain the query names")]
    DomainNotRecognized,
    /// The Supplier answered a status the profile gives no meaning to for this
    /// request, with the issue types its `OperationOutcome` carried, if it sent
    /// one (§2:3.78.4.1.3, Case 5 among them).
    #[error("the Supplier answered {status}")]
    Rejected {
        /// The HTTP status.
        status: StatusCode,
        /// The issue types of the `OperationOutcome`, in order.
        issues: Vec<IssueType>,
    },
    /// A page link points away from the Supplier the client is bound to, so
    /// the client does not follow it.
    #[error("the page link points away from the Supplier")]
    ForeignPage,
    /// No answer arrived within the timeout.
    #[error("the Supplier did not answer within the timeout")]
    Timeout,
    /// The request could not be sent or the answer could not be read.
    #[error("the Supplier could not be reached")]
    Transport(#[source] reqwest::Error),
    /// The answer does not hold to ITI-78.
    #[error("the Supplier's answer does not hold to ITI-78")]
    Malformed(#[from] Malformation),
    /// The audit recorder could not accept the request's audit record, so
    /// its answer is not used (feature `balp`, §2:3.78.5.1).
    #[cfg(feature = "balp")]
    #[error("the ITI-78 audit record could not be recorded")]
    Audit(#[source] crate::balp::AuditError),
}

/// How an answer departs from ITI-78 (§2:3.78.4.2.2, the Query Patient
/// Resource Response Message profile).
///
/// An entry is named by its position in the Bundle's `entry` list, never by
/// its content.
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
    /// The answer is a resource other than a Bundle.
    #[error("the answer is not a Bundle")]
    UnexpectedResource,
    /// The resource does not decode as FHIR R4.
    #[error("the resource does not decode as FHIR R4: {kind}")]
    Decode {
        /// Why it was refused.
        kind: fhir_types::codec::DecodeErrorKind,
    },
    /// The Bundle's `type` is not `searchset`.
    #[error("the Bundle is not a searchset")]
    NotSearchset,
    /// The Bundle has no `total`.
    #[error("the Bundle has no total")]
    NoTotal,
    /// The Bundle holds more matches than its `total` counts.
    #[error("the Bundle holds more matches than its total")]
    TotalBelowMatches,
    /// An entry has no `fullUrl`.
    #[error("entry {index} has no fullUrl")]
    NoFullUrl {
        /// The entry's position.
        index: usize,
    },
    /// An entry has no `resource`.
    #[error("entry {index} has no resource")]
    NoResource {
        /// The entry's position.
        index: usize,
    },
    /// An entry holds a resource that is neither a `Patient` nor an
    /// `OperationOutcome`; the client asks for no `_include`.
    #[error("entry {index} holds a resource ITI-78 does not answer with")]
    UnexpectedEntry {
        /// The entry's position.
        index: usize,
    },
    /// An entry's `search.score` is not a finite decimal.
    #[error("entry {index} has a score that is not a finite decimal")]
    Score {
        /// The entry's position.
        index: usize,
    },
    /// An entry's `match-grade` extension carries no code of its value set.
    #[error("entry {index} has a match-grade outside its value set")]
    MatchGrade {
        /// The entry's position.
        index: usize,
    },
    /// The `next` link is not a URL.
    #[error("the next link is not a URL")]
    NextLink,
}

/// A transport failure, with the request URL removed: a page link may hold
/// the query (§2:3.78.4.2.2.4).
pub(super) fn transport(error: reqwest::Error) -> PdqmError {
    let error = error.without_url();
    if error.is_timeout() {
        PdqmError::Timeout
    } else {
        PdqmError::Transport(error)
    }
}
