// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why a closed authorization question has no decision to give.
//!
//! No error here carries a BSN, a request body, or text Mitz wrote (a SOAP
//! fault reason, a status message): the request holds the BSN, and Mitz's
//! text may quote it. A data category is a code the afsprakenstelsel defines,
//! never a patient value, so an error may name one.

use http::StatusCode;

use super::question::DataCategory;

/// An argument the client refuses before anything is sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidInput {
    /// The endpoint is not an `https` URL with no userinfo, query or
    /// fragment, or, on the development path, not an `http` or `https` one.
    #[error(
        "the Mitz endpoint is not an https URL without userinfo, query or fragment (http is accepted for development only)"
    )]
    Endpoint,
    /// The BSN is empty.
    #[error("the BSN is empty")]
    EmptyBsn,
    /// A code (a care provider type, a data category or a role) is empty or
    /// has leading or trailing white space.
    #[error("a code is empty or has leading or trailing white space")]
    Code,
    /// An identifier root is not an ISO OID.
    #[error("an identifier root is not an ISO OID")]
    Oid,
    /// A professional's identification number is not 1 to 60 alphanumeric
    /// characters.
    #[error("a professional's identification number is not 1 to 60 alphanumeric characters")]
    ProfessionalNumber,
    /// The question asks about no data category.
    #[error("the question asks about no data category")]
    NoCategory,
    /// The question asks about one data category twice.
    #[error("the question asks about data category {0} twice")]
    DuplicateCategory(DataCategory),
    /// The question asks about more data categories than the client sends in
    /// one question.
    #[error("the question asks about more than {limit} data categories")]
    TooManyCategories {
        /// The limit.
        limit: usize,
    },
}

/// The HTTP client of a [`MitzClient`](super::MitzClient) could not be
/// built.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClientError {
    /// The endpoint is refused.
    #[error("the Mitz endpoint is refused")]
    Endpoint(#[source] InvalidInput),
    /// The HTTP client could not be built from the builder given.
    #[error("the HTTP client for Mitz could not be built")]
    Build(#[source] reqwest::Error),
}

/// Why a closed authorization question did not end in a decision for every
/// data category it asked about.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MitzError {
    /// Mitz answered a SOAP 1.2 fault.
    #[error("Mitz answered a SOAP fault ({code})")]
    Fault {
        /// The fault's code.
        code: FaultCode,
        /// The HTTP status the fault came with.
        status: StatusCode,
    },
    /// Mitz answered an HTTP status with no SOAP message.
    #[error("Mitz answered {status}")]
    Rejected {
        /// The HTTP status.
        status: StatusCode,
    },
    /// No answer arrived within the timeout.
    #[error("Mitz did not answer within the timeout")]
    Timeout,
    /// The request could not be sent or the answer could not be read.
    #[error("Mitz could not be reached")]
    Transport(#[source] reqwest::Error),
    /// The request could not be written.
    #[error("the closed authorization question could not be written")]
    Encode(#[source] std::io::Error),
    /// Mitz could not determine whether consent is recorded for this data
    /// category (`Indeterminate`).
    #[error("Mitz answered Indeterminate for data category {category}")]
    Indeterminate {
        /// The data category.
        category: DataCategory,
    },
    /// Mitz answered `NotApplicable` for this data category, which the
    /// closed authorization question does not define.
    #[error("Mitz answered NotApplicable for data category {category}")]
    NotApplicable {
        /// The data category.
        category: DataCategory,
    },
    /// The answer does not hold to the closed authorization question.
    #[error("Mitz's answer does not hold to the closed authorization question")]
    Malformed(#[from] Malformation),
}

impl MitzError {
    /// The HTTP status Mitz answered with, when it answered at all.
    ///
    /// An answer read under `200` that is not a decision for every category
    /// ([`MitzError::Indeterminate`], [`MitzError::NotApplicable`],
    /// [`MitzError::Malformed`]) reports `200`; a timeout, a transport
    /// failure and a request that could not be written report none.
    #[must_use]
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Fault { status, .. } | Self::Rejected { status } => Some(*status),
            Self::Indeterminate { .. } | Self::NotApplicable { .. } | Self::Malformed(_) => {
                Some(StatusCode::OK)
            }
            Self::Timeout | Self::Transport(_) | Self::Encode(_) => None,
        }
    }
}

/// The code of a SOAP 1.2 fault (SOAP 1.2 Part 1 §5.4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FaultCode {
    /// `Sender`: the message was incorrectly formed or carried wrong
    /// information.
    Sender,
    /// `Receiver`: the message could not be processed for reasons of the
    /// receiver.
    Receiver,
    /// Any other code, its text not kept.
    Other,
}

impl FaultCode {
    /// The fault code a `Code/Value` text names, read by its local part.
    pub(super) fn of(text: &str) -> Self {
        match text.rsplit(':').next().unwrap_or_default() {
            "Sender" => Self::Sender,
            "Receiver" => Self::Receiver,
            _ => Self::Other,
        }
    }
}

impl std::fmt::Display for FaultCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Sender => "Sender",
            Self::Receiver => "Receiver",
            Self::Other => "another code",
        })
    }
}

/// How an answer departs from the closed authorization question: a SOAP 1.2
/// envelope whose body holds one XACML 3.0 `Response` with one `Result` per
/// data category asked about.
///
/// A `Result` is named by its position in the answer, never by its content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Malformation {
    /// The answer's media type is not SOAP 1.2.
    #[error("the answer is not application/soap+xml")]
    NotSoap,
    /// The answer is longer than the client reads.
    #[error("the answer exceeds {limit} bytes")]
    TooLarge {
        /// The limit, in bytes.
        limit: usize,
    },
    /// The answer is not well-formed XML.
    #[error("the answer is not well-formed XML (byte {position})")]
    NotXml {
        /// The byte offset of the syntax error.
        position: u64,
    },
    /// The answer declares a document type, which the client never reads.
    #[error("the answer declares a document type")]
    DocumentType,
    /// The answer nests elements deeper than the client reads.
    #[error("the answer nests deeper than {limit} elements")]
    TooDeep {
        /// The limit.
        limit: usize,
    },
    /// The answer is not a SOAP 1.2 envelope with a body.
    #[error("the answer is not a SOAP 1.2 envelope with a body")]
    NotAnEnvelope,
    /// The body holds no XACML 3.0 `Response`, or more than one.
    #[error("the body holds no single XACML 3.0 Response")]
    NoResponse,
    /// A `Result` carries no single `Decision`.
    #[error("result {result} carries no single Decision")]
    NoDecision {
        /// The result's position.
        result: usize,
    },
    /// A `Result`'s `Decision` is not an XACML 3.0 decision.
    #[error("result {result} carries an unknown Decision")]
    UnknownDecision {
        /// The result's position.
        result: usize,
    },
    /// A `Result` names no single Mitz data category.
    #[error("result {result} names no single data category")]
    NoCategory {
        /// The result's position.
        result: usize,
    },
    /// A `Result` is about a data category the question did not ask about.
    #[error("result {result} is about a data category not asked about")]
    UnaskedCategory {
        /// The result's position.
        result: usize,
    },
    /// Two `Result`s are about one data category.
    #[error("result {result} repeats a data category")]
    DuplicateResult {
        /// The result's position.
        result: usize,
    },
    /// No `Result` is about a data category the question asked about.
    #[error("no result is about data category {0}")]
    MissingResult(DataCategory),
    /// A `Result` is not about the patient asked about.
    #[error("result {result} is not about the patient asked about")]
    OtherPatient {
        /// The result's position.
        result: usize,
    },
    /// A `Result` is not about the data holder asked about.
    #[error("result {result} is not about the data holder asked about")]
    OtherHolder {
        /// The result's position.
        result: usize,
    },
}

/// A transport failure, with the request URL removed.
pub(super) fn transport(error: reqwest::Error) -> MitzError {
    let error = error.without_url();
    if error.is_timeout() {
        MitzError::Timeout
    } else {
        MitzError::Transport(error)
    }
}
