// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Reading a Cross Gateway Patient Discovery Request on the Responding
//! Gateway side: a `PRPA_IN201305UV02` in a SOAP 1.2 envelope with its
//! WS-Addressing headers (ITI TF-2 §3.55.4.1, §3.55.6, Appendix V).
//!
//! The query parameters are those of Table 3.55.4.1.2.2-1. A request that
//! names neither a `livingSubjectId` nor both a `livingSubjectName` and a
//! `livingSubjectBirthTime` is refused, since §3.55.4.1.2.1 makes the name
//! and the birth time required unless an identifier is given.

use std::fmt;

use secrecy::SecretString;

use super::super::error::InvalidInput;
use super::super::identifier::{HomeCommunityId, Oid, PatientIdentifier};
use super::super::request::ProcessingCode;
use ihe_atna::redact::REDACTED;

mod parse;

/// The most bytes of a request the gateway reads (no specification governs
/// this: our own design, room for every parameter many times over).
pub const LIMIT: usize = 1024 * 1024;

/// The deepest element nesting the gateway reads (no specification governs
/// this: our own design; a request nests about a dozen deep).
const DEPTH: usize = 64;

/// Why a request is not one the Responding Gateway answers.
///
/// No variant carries a value from the request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Refusal {
    /// The request's media type is not `application/soap+xml`.
    #[error("the request is not application/soap+xml")]
    NotSoap,
    /// The request is longer than the gateway reads.
    #[error("the request exceeds {limit} bytes")]
    TooLarge {
        /// The limit, in bytes.
        limit: usize,
    },
    /// The request is not well-formed XML.
    #[error("the request is not well-formed XML (byte {position})")]
    NotXml {
        /// The byte offset of the fault.
        position: u64,
    },
    /// The request carries a document type declaration, which a SOAP
    /// message must not (SOAP 1.2 Part 1 §5).
    #[error("the request carries a document type declaration")]
    DocumentType,
    /// The request nests deeper than the gateway reads.
    #[error("the request nests deeper than {limit} elements")]
    TooDeep {
        /// The limit, in elements.
        limit: usize,
    },
    /// The request is not a SOAP 1.2 envelope with a body.
    #[error("the request is not a SOAP 1.2 envelope with a body")]
    NotAnEnvelope,
    /// The WS-Addressing `Action` is not the ITI-55 request's (§3.55.6.1.1).
    #[error("the WS-Addressing action is not the ITI-55 request's")]
    WrongAction,
    /// The request has no WS-Addressing `MessageID` (Appendix V).
    #[error("the request has no WS-Addressing MessageID")]
    NoMessageId,
    /// The body holds no `PRPA_IN201305UV02`, or its `interactionId` is
    /// another (Table 3.55.4.1.2.3-1).
    #[error("the SOAP body holds no PRPA_IN201305UV02 query")]
    WrongInteraction,
    /// The transmission wrapper has no `id`.
    #[error("the transmission wrapper has no id")]
    NoTransmissionId,
    /// The query has no `queryId` (Table 3.55.4.1.2.2-1).
    #[error("the query has no queryId")]
    NoQueryId,
    /// The query names neither a patient identifier nor both a name and a
    /// birth time (§3.55.4.1.2.1).
    #[error("the query names neither an identifier nor a name and a birth time")]
    TooFewParameters,
    /// An identifier, a device or a community is not an ISO OID, or an
    /// identifier value is empty.
    #[error("an identifier of the query is not usable")]
    Identifier(#[from] InvalidInput),
}

/// How soon the answer is wanted (`responsePriorityCode`,
/// Table 3.55.4.1.2.2-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// `I`, an immediate answer.
    Immediate,
    /// `D`, the Deferred Response Option (§3.55.6.2).
    Deferred,
}

/// A person name (`PN`) of the query: its given names and family names.
///
/// `Debug` shows how many parts the name has, never a part.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PersonName {
    /// The given names, in order.
    pub given: Vec<String>,
    /// The family names, in order.
    pub family: Vec<String>,
}

impl fmt::Debug for PersonName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PersonName")
            .field("given", &self.given.len())
            .field("family", &self.family.len())
            .finish()
    }
}

/// A postal address (`AD`) of the query.
///
/// `Debug` shows whether each part is present, never a part.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PostalAddress {
    /// The street address lines.
    pub lines: Vec<String>,
    /// The city.
    pub city: Option<String>,
    /// The state or province.
    pub state: Option<String>,
    /// The postal code.
    pub postal_code: Option<String>,
    /// The country.
    pub country: Option<String>,
}

impl fmt::Debug for PostalAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostalAddress")
            .field("lines", &self.lines.len())
            .field("city", &self.city.as_ref().map(|_| REDACTED))
            .field("postal_code", &self.postal_code.as_ref().map(|_| REDACTED))
            .finish_non_exhaustive()
    }
}

/// When the patient was born, as the query states it: a moment, or a range
/// (`LivingSubjectBirthTime`, §3.55.4.1.2.1).
///
/// The values are HL7 v3 `TS` texts, `YYYY[MM[DD[HH[MM[SS]]]]]` with an
/// optional zone, as the request wrote them. `Debug` redacts them.
#[derive(Clone, PartialEq, Eq)]
pub enum BirthTime {
    /// One moment, at the precision written.
    At(String),
    /// A range, either end open.
    Between {
        /// The low end.
        low: Option<String>,
        /// The high end.
        high: Option<String>,
    },
}

impl fmt::Debug for BirthTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::At(_) => f.write_str("At(***)"),
            Self::Between { .. } => f.write_str("Between(***)"),
        }
    }
}

/// The query parameters of Table 3.55.4.1.2.2-1.
///
/// `Debug` shows only how many values each parameter holds.
#[derive(Clone, Default)]
pub struct Parameters {
    /// `LivingSubjectName`: alternative names, combined with "or".
    pub names: Vec<PersonName>,
    /// `LivingSubjectAdministrativeGender`: the HL7 v3 code.
    pub gender: Option<String>,
    /// `LivingSubjectBirthTime`.
    pub birth_time: Option<BirthTime>,
    /// `PatientAddress`.
    pub addresses: Vec<PostalAddress>,
    /// `LivingSubjectId`: the identifiers, the shared one among them.
    pub ids: Vec<PatientIdentifier>,
    /// `LivingSubjectBirthPlaceAddress`.
    pub birth_place_address: Option<PostalAddress>,
    /// `LivingSubjectBirthPlaceName`.
    pub birth_place_name: Option<String>,
    /// `MothersMaidenName`.
    pub mothers_maiden_name: Option<PersonName>,
    /// `PatientTelecom`: the `TEL` values, combined with "or".
    pub telecom: Vec<String>,
    /// `PrincipalCareProviderId`: the care providers' identifiers.
    pub care_providers: Vec<(Oid, Option<String>)>,
    /// `MinimumDegreeMatch`: the least quality of match, 0 to 100.
    pub minimum_degree_match: Option<u8>,
}

impl fmt::Debug for Parameters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parameters")
            .field("names", &self.names.len())
            .field("gender", &self.gender.as_ref().map(|_| REDACTED))
            .field("birth_time", &self.birth_time.as_ref().map(|_| REDACTED))
            .field("addresses", &self.addresses.len())
            .field("ids", &self.ids)
            .field("telecom", &self.telecom.len())
            .field("minimum_degree_match", &self.minimum_degree_match)
            .finish_non_exhaustive()
    }
}

/// A Cross Gateway Patient Discovery Request as the Responding Gateway read
/// it.
#[derive(Debug, Clone)]
pub struct ReceivedQuery {
    /// The WS-Addressing `MessageID`, which the answer's `RelatesTo` names.
    pub message_id: String,
    /// The WS-Addressing `ReplyTo` address when it is not the anonymous one:
    /// the request asks for the Asynchronous Web Services Exchange
    /// (Appendix V.5).
    pub reply_to: Option<String>,
    /// The transmission wrapper's `id`: its `root` and `extension`, which
    /// the answer's acknowledgement names as its target message.
    pub transmission: (String, Option<String>),
    /// Whether the message belongs to production, training or debugging.
    pub processing: ProcessingCode,
    /// The sender's device.
    pub sender_device: Option<Oid>,
    /// The initiating community, the sender device's organization
    /// (§3.55.4.1.2.4).
    pub sender_community: Option<HomeCommunityId>,
    /// The receiver's device.
    pub receiver_device: Option<Oid>,
    /// The community the initiating gateway wants results from, the
    /// receiver device's organization (§3.55.4.1.2.5).
    pub target_community: Option<HomeCommunityId>,
    /// The assigning authority of the identifier to use in a reverse Cross
    /// Gateway Query, the `authorOrPerformer` device (§3.55.4.1.2.4).
    pub reverse_authority: Option<Oid>,
    /// The `queryId`: its `root` and `extension`.
    pub query_id: (String, Option<String>),
    /// How soon the answer is wanted.
    pub priority: Priority,
    /// The `CorrelationTimeToLive` header, an `xs:duration`, when present
    /// (§3.55.4.1.2).
    pub correlation_time_to_live: Option<String>,
    /// The query parameters.
    pub parameters: Parameters,
    /// The `queryByParameter` element as the request wrote it, which the
    /// answer copies (Table 3.55.4.2.2.3-1) and the audit message records
    /// (§3.55.5.1.2). It names the patient.
    pub query_by_parameter: SecretString,
}

/// Reads the request with media type `media` and `body`.
///
/// # Errors
///
/// A [`Refusal`] for a request that is not a well-formed ITI-55 query.
pub fn read(media: Option<&str>, body: &[u8]) -> Result<ReceivedQuery, Refusal> {
    let soap = media.is_some_and(|media| {
        media
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .eq_ignore_ascii_case("application/soap+xml")
    });
    if !soap {
        return Err(Refusal::NotSoap);
    }
    if body.len() > LIMIT {
        return Err(Refusal::TooLarge { limit: LIMIT });
    }
    let text = std::str::from_utf8(body).map_err(|error| Refusal::NotXml {
        position: u64::try_from(error.valid_up_to()).unwrap_or(u64::MAX),
    })?;
    let mut collected = parse::Collected::default();
    parse::parse(text, &mut collected)?;
    collected.finish(text)
}
