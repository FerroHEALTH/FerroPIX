// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The streaming reader behind [`super::read`]: the elements of the SOAP
//! envelope, the transmission and control act wrappers and the query
//! parameters, collected in one pass and checked once at the end.

use quick_xml::NsReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use secrecy::SecretString;

use super::super::super::identifier::{HomeCommunityId, Oid, PatientIdentifier};
use super::super::super::request::{ANONYMOUS, ProcessingCode};
use super::super::super::{ACTION, HL7, SOAP, WSA};
use super::{
    BirthTime, DEPTH, Parameters, PersonName, PostalAddress, Priority, ReceivedQuery, Refusal,
};

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
pub(super) struct Collected {
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
pub(super) fn parse(text: &str, collected: &mut Collected) -> Result<(), Refusal> {
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
    pub(super) fn finish(self, text: &str) -> Result<ReceivedQuery, Refusal> {
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
