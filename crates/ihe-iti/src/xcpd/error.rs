// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why an ITI-55 exchange has no discovery to give.
//!
//! No error here carries an identifier value, a request body or text the
//! responding gateway wrote (a SOAP fault reason, an acknowledgement detail's
//! text): the request holds the patient identifier, and the gateway's text
//! may quote it. A coded value the transaction defines is kept as its code.

use http::StatusCode;

/// An argument the client refuses before anything is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidInput {
    /// A device, an assigning authority or a home community is not an ISO
    /// OID (ITI TF-2 Appendix O).
    #[error("an identifier root is not an ISO OID")]
    Oid,
    /// The patient identifier value is empty.
    #[error("the patient identifier value is empty")]
    EmptyValue,
    /// The responding gateway's endpoint is not an `https` URL without a
    /// fragment, or, on the development path, not an `http` or `https` one.
    #[error(
        "the responding gateway endpoint is not an https URL without a fragment (http is accepted for development only)"
    )]
    Endpoint,
    /// The XUA assertion is not one SAML 2.0 `Assertion` element.
    #[error("the XUA assertion is not one SAML 2.0 Assertion element")]
    Assertion,
}

/// Why an ITI-55 exchange did not end in a discovery.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum XcpdError {
    /// The responding gateway answered a SOAP 1.2 fault, a transmission
    /// error (§3.55.4.2.3).
    #[error("the responding gateway answered a SOAP fault ({code})")]
    Fault {
        /// The fault's code.
        code: FaultCode,
        /// The HTTP status the fault came with.
        status: StatusCode,
    },
    /// The responding gateway could not satisfy the request, Case 5 of
    /// §3.55.4.2.3: an `AE` or `AR` acknowledgement or query response, with
    /// the detected issues it coded (§3.55.4.2.2.7).
    #[error("the responding gateway could not satisfy the request")]
    ApplicationError {
        /// The issues the response coded, in order.
        issues: Vec<DetectedIssue>,
    },
    /// The responding gateway refused the query's parameters: a `QE` query
    /// response.
    #[error("the responding gateway refused the query parameters")]
    QueryRefused,
    /// The responding gateway answered an HTTP status with no SOAP message
    /// the transaction gives a meaning.
    #[error("the responding gateway answered {status}")]
    Rejected {
        /// The HTTP status.
        status: StatusCode,
    },
    /// No answer arrived within the timeout.
    #[error("the responding gateway did not answer within the timeout")]
    Timeout,
    /// The request could not be sent or the answer could not be read.
    #[error("the responding gateway could not be reached")]
    Transport(#[source] reqwest::Error),
    /// The request could not be written.
    #[error("the ITI-55 request could not be written")]
    Encode(#[source] std::io::Error),
    /// The answer does not hold to ITI-55.
    #[error("the responding gateway's answer does not hold to ITI-55")]
    Malformed(#[from] Malformation),
    /// The audit recorder could not accept the exchange's audit message, so
    /// the answer is not used (ITI TF-2 §3.55.5.1).
    #[error("the ITI-55 audit message could not be recorded")]
    Audit(#[source] super::audit::AuditError),
}

impl XcpdError {
    /// The HTTP status the responding gateway answered with, or `None` when
    /// no answer arrived or nothing was sent.
    ///
    /// A malformed answer, an application error and a refused query are read
    /// only from a `200`; any other status is [`XcpdError::Rejected`] unless it
    /// carries a SOAP fault.
    #[must_use]
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Fault { status, .. } | Self::Rejected { status } => Some(*status),
            Self::ApplicationError { .. } | Self::QueryRefused | Self::Malformed(_) => {
                Some(StatusCode::OK)
            }
            Self::Timeout | Self::Transport(_) | Self::Encode(_) | Self::Audit(_) => None,
        }
    }
}

/// The code of a SOAP 1.2 fault (SOAP 1.2 Part 1 §5.4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FaultCode {
    /// `VersionMismatch`.
    VersionMismatch,
    /// `MustUnderstand`: a header the gateway was required to understand,
    /// such as the WS-Addressing `Action` or the WS-Security header.
    MustUnderstand,
    /// `DataEncodingUnknown`.
    DataEncodingUnknown,
    /// `Sender`: the request was at fault.
    Sender,
    /// `Receiver`: the responding gateway was at fault.
    Receiver,
    /// A code SOAP 1.2 does not define.
    Other,
}

impl FaultCode {
    /// The code a fault's `Code/Value` names, by its local part.
    pub(super) fn of(value: &str) -> Self {
        let local = value.rsplit(':').next().unwrap_or(value).trim();
        match local {
            "VersionMismatch" => Self::VersionMismatch,
            "MustUnderstand" => Self::MustUnderstand,
            "DataEncodingUnknown" => Self::DataEncodingUnknown,
            "Sender" => Self::Sender,
            "Receiver" => Self::Receiver,
            _ => Self::Other,
        }
    }
}

impl std::fmt::Display for FaultCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::VersionMismatch => "VersionMismatch",
            Self::MustUnderstand => "MustUnderstand",
            Self::DataEncodingUnknown => "DataEncodingUnknown",
            Self::Sender => "Sender",
            Self::Receiver => "Receiver",
            Self::Other => "a code SOAP 1.2 does not define",
        })
    }
}

/// A problem the responding gateway coded in its response, from code system
/// `1.3.6.1.4.1.19376.1.2.27.3` (§3.55.4.2.2.7, Table 3.55.4.2.2.7-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DetectedIssue {
    /// `ResponderBusy`: the responder is overloaded.
    ResponderBusy,
    /// `AnswerNotAvailable`: human intervention may be needed.
    AnswerNotAvailable,
    /// `InternalError`: an internal error or inconsistency.
    InternalError,
    /// A code the table does not define, or one from another code system.
    Other,
}

impl DetectedIssue {
    /// The issue `code` names in `system`.
    pub(super) fn of(code: &str, system: Option<&str>) -> Self {
        if system != Some(super::ISSUE_SYSTEM) {
            return Self::Other;
        }
        match code {
            "ResponderBusy" => Self::ResponderBusy,
            "AnswerNotAvailable" => Self::AnswerNotAvailable,
            "InternalError" => Self::InternalError,
            _ => Self::Other,
        }
    }
}

/// How an answer departs from ITI-55 (§3.55.4.2, ITI TF-2 Appendix O and V).
///
/// An element is named by its role in the message, never by its content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Malformation {
    /// The answer's media type is not `application/soap+xml`.
    #[error("the answer is not application/soap+xml")]
    NotSoap,
    /// The answer is a multipart (MTOM) package, which an ITI-55 response
    /// carries no attachment to need.
    #[error("the answer is a multipart package")]
    Multipart,
    /// The answer is longer than the client reads.
    #[error("the answer exceeds {limit} bytes")]
    TooLarge {
        /// The limit, in bytes.
        limit: usize,
    },
    /// The answer is not well-formed XML.
    #[error("the answer is not well-formed XML (byte {position})")]
    NotXml {
        /// The byte offset of the fault.
        position: u64,
    },
    /// The answer carries a document type declaration, which a SOAP message
    /// must not (SOAP 1.2 Part 1 §5).
    #[error("the answer carries a document type declaration")]
    DocumentType,
    /// The answer nests deeper than the client reads.
    #[error("the answer nests deeper than {limit} elements")]
    TooDeep {
        /// The limit, in elements.
        limit: usize,
    },
    /// The answer is not a SOAP 1.2 envelope with a body.
    #[error("the answer is not a SOAP 1.2 envelope with a body")]
    NotAnEnvelope,
    /// The body holds neither a `PRPA_IN201306UV02` response nor a fault.
    #[error("the SOAP body holds neither an ITI-55 response nor a fault")]
    UnexpectedBody,
    /// The response's `interactionId` is not `PRPA_IN201306UV02`
    /// (Table 3.55.4.2.2.3-1).
    #[error("the response's interactionId is not PRPA_IN201306UV02")]
    WrongInteraction,
    /// The answer relates to another request than the one sent
    /// (WS-Addressing `RelatesTo`).
    #[error("the answer relates to another request")]
    RelatesToAnother,
    /// The transmission wrapper has no `acknowledgement` type code, or one
    /// HL7 v3 does not define.
    #[error("the response has no valid acknowledgement type code")]
    Acknowledgement,
    /// An accepted response has no `queryAck` response code, or one HL7 v3
    /// does not define.
    #[error("the response has no valid query response code")]
    QueryResponseCode,
    /// A `NF` (no match) response holds registration events (Case 4).
    #[error("a no-match response holds registration events")]
    MatchesWithoutMatch,
    /// An `OK` response holds no registration event and requests no
    /// further attribute, which none of the five cases of §3.55.4.2.3 is.
    #[error("an OK response holds neither a match nor a request for attributes")]
    EmptyMatch,
    /// A registration event names no `homeCommunityId`, which the responding
    /// gateway "shall specify ... within every `RegistrationEvent`"
    /// (§3.55.4.2.2.4).
    #[error("registration event {event} names no homeCommunityId")]
    NoHomeCommunity {
        /// The event's position, counted from 0.
        event: usize,
    },
    /// A registration event's `Patient` has no `id` (Table 3.55.4.2.2.2-1,
    /// `Patient.id [1..1]`).
    #[error("registration event {event} has no patient id")]
    NoPatientId {
        /// The event's position, counted from 0.
        event: usize,
    },
}
