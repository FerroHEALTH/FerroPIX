// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why a Localization Service search has no localization to give.
//!
//! No error here carries a request URL, a pseudonym, or text the service
//! wrote (an `OperationOutcome`'s `diagnostics`, a JSON parse snippet): the
//! request URL holds the pseudonym, and the service's text may quote it. A
//! record is named by its position in the answer, never by its content.

use http::StatusCode;

/// An argument the client refuses before anything is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidInput {
    /// The service's base URL is not an `http` or `https` URL without a query
    /// or a fragment.
    #[error(
        "the Localization Service base URL is not an http(s) URL without a query or a fragment"
    )]
    Base,
}

/// Why a Localization Service search did not end in a localization.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NviError {
    /// The service answered a status other than `200`.
    #[error("the Localization Service answered {status}")]
    Rejected {
        /// The HTTP status.
        status: StatusCode,
    },
    /// No answer arrived within the timeout.
    #[error("the Localization Service did not answer within the timeout")]
    Timeout,
    /// The request could not be sent or the answer could not be read.
    #[error("the Localization Service could not be reached")]
    Transport(#[source] reqwest::Error),
    /// The authorizer could not authenticate the request, so nothing was
    /// sent.
    #[error("the request to the Localization Service could not be authenticated")]
    Unauthenticated(#[source] crate::authorizer::AuthorizerError),
    /// The answer does not hold to the Localization Service search.
    #[error("the Localization Service's answer does not hold to the IG")]
    Malformed(#[from] Malformation),
}

/// How an answer departs from the Localization Service search: a FHIR R4
/// `searchset` Bundle of `nl-gf-localization-documentreference` records
/// about the patient asked for.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Malformation {
    /// The answer's media type is not FHIR JSON.
    #[error("the answer is not application/fhir+json")]
    NotFhirJson,
    /// An answer is longer than the client reads.
    #[error("the answer exceeds {limit} bytes")]
    TooLarge {
        /// The limit, in bytes.
        limit: usize,
    },
    /// The answer runs to more pages than the client follows.
    #[error("the answer runs to more than {limit} pages")]
    TooManyPages {
        /// The limit, in pages.
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
    /// The answer is not a FHIR R4 `Bundle`.
    #[error("the answer is not a Bundle")]
    NotABundle,
    /// The Bundle does not decode as FHIR R4.
    #[error("the Bundle does not decode as FHIR R4: {kind}")]
    Decode {
        /// Why it was refused.
        kind: fhir_types::codec::DecodeErrorKind,
    },
    /// The Bundle is not a `searchset`.
    #[error("the Bundle is not a searchset")]
    NotASearchset,
    /// The `next` link has no URL, or one outside the search endpoint, where
    /// the pseudonym would be sent.
    #[error("the next link is missing or leaves the search endpoint")]
    NextLink,
    /// An entry carries no resource.
    #[error("entry {index} carries no resource")]
    NoResource {
        /// The entry's position.
        index: usize,
    },
    /// An entry is neither a `DocumentReference` nor an `OperationOutcome`.
    #[error("entry {index} is not a localization record")]
    UnexpectedEntry {
        /// The entry's position.
        index: usize,
    },
    /// A record's `status` is not a `DocumentReference` status.
    #[error("entry {index} has no document reference status")]
    Status {
        /// The entry's position.
        index: usize,
    },
    /// A record's `subject` is not the pseudonymised BSN asked about.
    #[error("entry {index} is about another subject")]
    OtherSubject {
        /// The entry's position.
        index: usize,
    },
    /// A record's `type` is not the type asked about.
    #[error("entry {index} is of another type")]
    OtherType {
        /// The entry's position.
        index: usize,
    },
    /// A record's `custodian` is not one URA identifier.
    #[error("entry {index} names no custodian by URA")]
    NoCustodian {
        /// The entry's position.
        index: usize,
    },
}

/// A transport failure, with the request URL removed: it holds the
/// pseudonym.
pub(super) fn transport(error: reqwest::Error) -> NviError {
    let error = error.without_url();
    if error.is_timeout() {
        NviError::Timeout
    } else {
        NviError::Transport(error)
    }
}
