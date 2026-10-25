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

use quick_xml::NsReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use secrecy::SecretString;

use super::super::error::InvalidInput;
use super::super::identifier::{HomeCommunityId, Oid, PatientIdentifier};
use super::super::request::{ANONYMOUS, ProcessingCode};
use super::super::{ACTION, HL7, SOAP, WSA};
use ihe_atna::redact::REDACTED;

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

/// The local names a parameter element may carry, lowerCamel as the HL7 v3
/// XML ITS writes them; §3.55.4.1.2.4's example spells one `LivingSubjectId`.
fn parameter(local: &str) -> Option<&'static str> {
    let names = [
        "livingSubjectName",
        "livingSubjectAdministrativeGender",
        "livingSubjectBirthTime",
        "patientAddress",
        "livingSubjectId",
        "livingSubjectBirthPlaceAddress",
        "livingSubjectBirthPlaceName",
        "mothersMaidenName",
        "patientTelecom",
        "principalCareProviderId",
    ];
    names
        .iter()
        .copied()
        .find(|name| name.eq_ignore_ascii_case(local))
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
    let mut collected = Collected::default();
    parse(text, &mut collected)?;
    collected.finish(text)
}

/// Which namespace an element is in, of the ones the reader looks at.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ns {
    Soap,
    Wsa,
    Hl7,
    Other,
}

/// One open element: its namespace and local name.
type Open = (Ns, String);

/// What the reader collects.
#[derive(Default)]
struct Collected {
    envelope: bool,
    body: bool,
    query: bool,
    action: Option<String>,
    message_id: Option<String>,
    reply_to: Option<String>,
    correlation: Option<String>,
    interaction: Option<String>,
    transmission: Option<(String, Option<String>)>,
    processing: Option<String>,
    sender_device: Option<String>,
    sender_community: Option<String>,
    receiver_device: Option<String>,
    target_community: Option<String>,
    reverse_authority: Option<String>,
    query_id: Option<(String, Option<String>)>,
    priority: Option<String>,
    parameters: Parameters,
    name: Option<PersonName>,
    address: Option<PostalAddress>,
    birth: Option<(Option<String>, Option<String>, Option<String>)>,
    segment: Option<(usize, usize)>,
}

/// Collects what the request says, refusing what is not well-formed XML.
fn parse(text: &str, collected: &mut Collected) -> Result<(), Refusal> {
    let mut reader = NsReader::from_str(text);
    let mut open: Vec<Open> = Vec::new();
    let mut words = String::new();
    loop {
        let before = usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX);
        let (ns, event) = match reader.read_resolved_event() {
            Ok((namespace, event)) => (kind(&namespace), event),
            Err(_syntax) => {
                return Err(Refusal::NotXml {
                    position: reader.error_position(),
                });
            }
        };
        match event {
            Event::Start(element) => {
                if open.len() >= DEPTH {
                    return Err(Refusal::TooDeep { limit: DEPTH });
                }
                open.push((ns, local_name(&element)));
                opened(collected, &open, &element, before)?;
                words.clear();
            }
            Event::Empty(element) => {
                open.push((ns, local_name(&element)));
                opened(collected, &open, &element, before)?;
                closed(collected, &open, "", before);
                open.pop();
            }
            Event::End(_) => {
                let after = usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX);
                closed(collected, &open, &words, after);
                words.clear();
                open.pop();
            }
            Event::Text(text) => words.push_str(&text.xml10_content()),
            Event::CData(data) => words.push_str(&data.into_inner()),
            Event::GeneralRef(reference) => {
                let resolved = reference
                    .resolve_char_ref()
                    .ok()
                    .flatten()
                    .map(String::from)
                    .or_else(|| {
                        quick_xml::escape::resolve_predefined_entity(&reference.xml10_content())
                            .map(str::to_owned)
                    })
                    .ok_or(Refusal::NotXml {
                        position: reader.buffer_position(),
                    })?;
                words.push_str(&resolved);
            }
            Event::DocType(_) => return Err(Refusal::DocumentType),
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
        }
    }
    Ok(())
}

/// Which of the namespaces the reader looks at `namespace` is.
fn kind(namespace: &ResolveResult<'_>) -> Ns {
    match namespace {
        ResolveResult::Bound(ns) if ns.0 == SOAP => Ns::Soap,
        ResolveResult::Bound(ns) if ns.0 == WSA => Ns::Wsa,
        ResolveResult::Bound(ns) if ns.0 == HL7 => Ns::Hl7,
        _ => Ns::Other,
    }
}

fn local_name(element: &BytesStart<'_>) -> String {
    element.local_name().as_ref().to_owned()
}

/// The value of the attribute `name`, unescaped, if the element has it.
fn attribute(element: &BytesStart<'_>, name: &str) -> Result<Option<String>, Refusal> {
    let malformed = Refusal::NotXml { position: 0 };
    let Some(found) = element
        .try_get_attribute(name)
        .map_err(|_attribute| malformed.clone())?
    else {
        return Ok(None);
    };
    let value = found
        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
        .map_err(|_escape| malformed)?;
    Ok(Some(value.into_owned()))
}

/// The open elements below the query interaction, in the HL7 namespace, as
/// local names; `None` when the path leaves the interaction or the
/// namespace.
fn inside(open: &[Open]) -> Option<Vec<&str>> {
    let inner = open.get(3..)?;
    inner
        .iter()
        .map(|(ns, local)| (*ns == Ns::Hl7).then_some(local.as_str()))
        .collect()
}

/// What the element just opened says, given the elements open around it.
fn opened(
    collected: &mut Collected,
    open: &[Open],
    element: &BytesStart<'_>,
    at: usize,
) -> Result<(), Refusal> {
    let names: Vec<(Ns, &str)> = open
        .iter()
        .map(|(ns, local)| (*ns, local.as_str()))
        .collect();
    match names.as_slice() {
        [(Ns::Soap, "Envelope")] => collected.envelope = true,
        [(_, _)] => return Err(Refusal::NotAnEnvelope),
        [(Ns::Soap, "Envelope"), (Ns::Soap, "Body")] => collected.body = true,
        [(Ns::Soap, "Envelope"), (Ns::Soap, "Body"), child] => match child {
            (Ns::Hl7, "PRPA_IN201305UV02") if !collected.query => collected.query = true,
            _ => return Err(Refusal::WrongInteraction),
        },
        _ => {}
    }
    if !collected.query {
        return Ok(());
    }
    let Some(path) = inside(open) else {
        return Ok(());
    };
    let id = |element: &BytesStart<'_>| -> Result<Option<String>, Refusal> {
        attribute(element, "root")
    };
    match path.as_slice() {
        ["id"] => {
            if let Some(root) = id(element)? {
                collected.transmission = Some((root, attribute(element, "extension")?));
            }
        }
        ["interactionId"] => collected.interaction = attribute(element, "extension")?,
        ["processingCode"] => collected.processing = attribute(element, "code")?,
        ["sender", "device", "id"] => collected.sender_device = id(element)?,
        [
            "sender",
            "device",
            "asAgent",
            "representedOrganization",
            "id",
        ] => {
            collected.sender_community = id(element)?;
        }
        ["receiver", "device", "id"] => collected.receiver_device = id(element)?,
        [
            "receiver",
            "device",
            "asAgent",
            "representedOrganization",
            "id",
        ] => {
            collected.target_community = id(element)?;
        }
        [
            "controlActProcess",
            "authorOrPerformer",
            "assignedDevice",
            "id",
        ] => {
            collected.reverse_authority = id(element)?;
        }
        ["controlActProcess", "queryByParameter"] => collected.segment = Some((at, at)),
        ["controlActProcess", "queryByParameter", "queryId"] => {
            if let Some(root) = id(element)? {
                collected.query_id = Some((root, attribute(element, "extension")?));
            }
        }
        [
            "controlActProcess",
            "queryByParameter",
            "responsePriorityCode",
        ] => {
            collected.priority = attribute(element, "code")?;
        }
        [
            "controlActProcess",
            "queryByParameter",
            "matchCriterionList",
            "minimumDegreeMatch",
            "value",
        ] => {
            collected.parameters.minimum_degree_match = attribute(element, "value")?
                .and_then(|value| value.trim().parse::<u8>().ok())
                .filter(|value| *value <= 100);
        }
        [
            "controlActProcess",
            "queryByParameter",
            "parameterList",
            name,
            rest @ ..,
        ] => {
            parameter_opened(collected, parameter(name), rest, element)?;
        }
        _ => {}
    }
    Ok(())
}

/// What an element inside the parameter `name` says as it opens.
fn parameter_opened(
    collected: &mut Collected,
    name: Option<&str>,
    rest: &[&str],
    element: &BytesStart<'_>,
) -> Result<(), Refusal> {
    let parameters = &mut collected.parameters;
    match (name, rest) {
        (Some("livingSubjectName" | "mothersMaidenName"), ["value"]) => {
            collected.name = Some(PersonName::default());
        }
        (Some("patientAddress" | "livingSubjectBirthPlaceAddress"), ["value"]) => {
            collected.address = Some(PostalAddress::default());
        }
        (Some("livingSubjectAdministrativeGender"), ["value"]) => {
            parameters.gender = attribute(element, "code")?;
        }
        (Some("livingSubjectBirthTime"), ["value"]) => {
            collected.birth = Some((attribute(element, "value")?, None, None));
        }
        (Some("livingSubjectBirthTime"), ["value", "low"]) => {
            if let Some(birth) = collected.birth.as_mut() {
                birth.1 = attribute(element, "value")?;
            }
        }
        (Some("livingSubjectBirthTime"), ["value", "high"]) => {
            if let Some(birth) = collected.birth.as_mut() {
                birth.2 = attribute(element, "value")?;
            }
        }
        (Some("livingSubjectId"), ["value"]) => {
            if let Some(root) = attribute(element, "root")? {
                let value = attribute(element, "extension")?.unwrap_or_default();
                parameters.ids.push(PatientIdentifier::new(
                    Oid::new(&root)?,
                    SecretString::from(value),
                )?);
            }
        }
        (Some("patientTelecom"), ["value"]) => {
            if let Some(value) = attribute(element, "value")? {
                parameters.telecom.push(value);
            }
        }
        (Some("principalCareProviderId"), ["value"]) => {
            if let Some(root) = attribute(element, "root")? {
                parameters
                    .care_providers
                    .push((Oid::new(&root)?, attribute(element, "extension")?));
            }
        }
        _ => {}
    }
    Ok(())
}

/// What an element says as it closes, with the `words` it held, at byte
/// `at` past its end.
fn closed(collected: &mut Collected, open: &[Open], words: &str, at: usize) {
    let names: Vec<(Ns, &str)> = open
        .iter()
        .map(|(ns, local)| (*ns, local.as_str()))
        .collect();
    let words = words.trim();
    match names.as_slice() {
        [
            (Ns::Soap, "Envelope"),
            (Ns::Soap, "Header"),
            (Ns::Wsa, "Action"),
        ] => {
            collected.action = Some(words.to_owned());
        }
        [
            (Ns::Soap, "Envelope"),
            (Ns::Soap, "Header"),
            (Ns::Wsa, "MessageID"),
        ] => {
            collected.message_id = Some(words.to_owned());
        }
        [
            (Ns::Soap, "Envelope"),
            (Ns::Soap, "Header"),
            (Ns::Wsa, "ReplyTo"),
            (Ns::Wsa, "Address"),
        ] => collected.reply_to = Some(words.to_owned()),
        [
            (Ns::Soap, "Envelope"),
            (Ns::Soap, "Header"),
            (_, "CorrelationTimeToLive"),
        ] => {
            collected.correlation = Some(words.to_owned());
        }
        _ => {}
    }
    if !collected.query {
        return;
    }
    let Some(path) = inside(open) else {
        return;
    };
    match path.as_slice() {
        ["controlActProcess", "queryByParameter"] => {
            if let Some(segment) = collected.segment.as_mut() {
                segment.1 = at;
            }
        }
        [
            "controlActProcess",
            "queryByParameter",
            "parameterList",
            name,
            rest @ ..,
        ] => {
            parameter_closed(collected, parameter(name), rest, words);
        }
        _ => {}
    }
}

/// What an element inside the parameter `name` says as it closes.
fn parameter_closed(collected: &mut Collected, name: Option<&str>, rest: &[&str], words: &str) {
    let text = (!words.is_empty()).then(|| words.to_owned());
    match (name, rest) {
        (Some("livingSubjectName" | "mothersMaidenName"), ["value", part]) => {
            if let (Some(name), Some(text)) = (collected.name.as_mut(), text) {
                match *part {
                    "given" => name.given.push(text),
                    "family" => name.family.push(text),
                    _ => {}
                }
            }
        }
        (Some("livingSubjectName"), ["value"]) => {
            if let Some(name) = collected.name.take() {
                collected.parameters.names.push(name);
            }
        }
        (Some("mothersMaidenName"), ["value"]) => {
            collected.parameters.mothers_maiden_name = collected.name.take();
        }
        (Some("patientAddress" | "livingSubjectBirthPlaceAddress"), ["value", part]) => {
            if let (Some(address), Some(text)) = (collected.address.as_mut(), text) {
                match *part {
                    "streetAddressLine" => address.lines.push(text),
                    "city" => address.city = Some(text),
                    "state" => address.state = Some(text),
                    "postalCode" => address.postal_code = Some(text),
                    "country" => address.country = Some(text),
                    _ => {}
                }
            }
        }
        (Some("patientAddress"), ["value"]) => {
            if let Some(address) = collected.address.take() {
                collected.parameters.addresses.push(address);
            }
        }
        (Some("livingSubjectBirthPlaceAddress"), ["value"]) => {
            collected.parameters.birth_place_address = collected.address.take();
        }
        (Some("livingSubjectBirthPlaceName"), ["value"]) => {
            collected.parameters.birth_place_name = text;
        }
        (Some("livingSubjectBirthTime"), ["value"]) => {
            collected.parameters.birth_time = match collected.birth.take() {
                Some((Some(at), _, _)) => Some(BirthTime::At(at)),
                Some((None, low, high)) if low.is_some() || high.is_some() => {
                    Some(BirthTime::Between { low, high })
                }
                _ => None,
            };
        }
        _ => {}
    }
}

impl Collected {
    /// The query the request holds, refusing what ITI-55 requires and the
    /// request lacks.
    fn finish(self, text: &str) -> Result<ReceivedQuery, Refusal> {
        if !self.envelope || !self.body {
            return Err(Refusal::NotAnEnvelope);
        }
        if self.action.as_deref() != Some(ACTION) {
            return Err(Refusal::WrongAction);
        }
        let message_id = self
            .message_id
            .filter(|id| !id.is_empty())
            .ok_or(Refusal::NoMessageId)?;
        if !self.query || self.interaction.as_deref() != Some("PRPA_IN201305UV02") {
            return Err(Refusal::WrongInteraction);
        }
        let transmission = self.transmission.ok_or(Refusal::NoTransmissionId)?;
        let query_id = self.query_id.ok_or(Refusal::NoQueryId)?;
        let parameters = self.parameters;
        let demographics = !parameters.names.is_empty() && parameters.birth_time.is_some();
        if parameters.ids.is_empty() && !demographics {
            return Err(Refusal::TooFewParameters);
        }
        let segment = self
            .segment
            .and_then(|(start, end)| text.get(start..end))
            .map(|segment| segment.trim_start().to_owned())
            .unwrap_or_default();
        let oid = |text: Option<String>| text.map(|text| Oid::new(&text)).transpose();
        Ok(ReceivedQuery {
            message_id,
            reply_to: self
                .reply_to
                .filter(|address| !address.is_empty() && address != ANONYMOUS),
            transmission,
            processing: match self.processing.as_deref() {
                Some("T") => ProcessingCode::Training,
                Some("D") => ProcessingCode::Debugging,
                _ => ProcessingCode::Production,
            },
            sender_device: oid(self.sender_device)?,
            sender_community: oid(self.sender_community)?.map(HomeCommunityId::new),
            receiver_device: oid(self.receiver_device)?,
            target_community: oid(self.target_community)?.map(HomeCommunityId::new),
            reverse_authority: oid(self.reverse_authority)?,
            query_id,
            priority: match self.priority.as_deref() {
                Some("D") => Priority::Deferred,
                _ => Priority::Immediate,
            },
            correlation_time_to_live: self.correlation.filter(|ttl| !ttl.is_empty()),
            parameters,
            query_by_parameter: SecretString::from(segment),
        })
    }
}
