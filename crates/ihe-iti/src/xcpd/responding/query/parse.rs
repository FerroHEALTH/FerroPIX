// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The streaming reader behind [`super::read`]: the elements of the SOAP
//! envelope, the transmission and control act wrappers and the query
//! parameters, collected in one pass and checked once at the end.

use secrecy::SecretString;

use super::super::super::ACTION;
use super::super::super::identifier::{HomeCommunityId, Oid, PatientIdentifier};
use super::super::super::request::ProcessingCode;
use super::super::super::v3::{self, Ns};
use super::{BirthTime, Parameters, PersonName, PostalAddress, Priority, ReceivedQuery, Refusal};
use crate::soap::{ANONYMOUS, Element, Visitor};

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

/// What the reader collects.
#[derive(Default)]
pub(super) struct Collected {
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

/// The open elements below the query interaction, in the HL7 namespace, as
/// local names; `None` when the path leaves the interaction or the
/// namespace.
fn inside<'a>(open: &[(Ns, &'a str)]) -> Option<Vec<&'a str>> {
    let inner = open.get(3..)?;
    inner
        .iter()
        .map(|(ns, local)| (*ns == Ns::Hl7).then_some(*local))
        .collect()
}

impl Visitor for Collected {
    type Namespace = Ns;
    type Error = Refusal;
    const SOAP: Ns = Ns::Soap;
    const NAMESPACES: &'static [(&'static str, Ns)] = v3::NAMESPACES;
    const OTHER: Ns = Ns::Other;

    fn opened(&mut self, open: &[(Ns, &str)], element: &Element<'_>) -> Result<(), Refusal> {
        if let [(Ns::Soap, "Envelope"), (Ns::Soap, "Body"), child] = open {
            match child {
                (Ns::Hl7, "PRPA_IN201305UV02") if !self.query => self.query = true,
                _ => return Err(Refusal::WrongInteraction),
            }
        }
        if !self.query {
            return Ok(());
        }
        let Some(path) = inside(open) else {
            return Ok(());
        };
        let id = |element: &Element<'_>| -> Result<Option<String>, Refusal> {
            Ok(element.attribute("root")?)
        };
        match path.as_slice() {
            ["id"] => {
                if let Some(root) = id(element)? {
                    self.transmission = Some((root, element.attribute("extension")?));
                }
            }
            ["interactionId"] => self.interaction = element.attribute("extension")?,
            ["processingCode"] => self.processing = element.attribute("code")?,
            ["sender", "device", "id"] => self.sender_device = id(element)?,
            [
                "sender",
                "device",
                "asAgent",
                "representedOrganization",
                "id",
            ] => {
                self.sender_community = id(element)?;
            }
            ["receiver", "device", "id"] => self.receiver_device = id(element)?,
            [
                "receiver",
                "device",
                "asAgent",
                "representedOrganization",
                "id",
            ] => {
                self.target_community = id(element)?;
            }
            [
                "controlActProcess",
                "authorOrPerformer",
                "assignedDevice",
                "id",
            ] => {
                self.reverse_authority = id(element)?;
            }
            ["controlActProcess", "queryByParameter"] => {
                self.segment = Some((element.at(), element.at()));
            }
            ["controlActProcess", "queryByParameter", "queryId"] => {
                if let Some(root) = id(element)? {
                    self.query_id = Some((root, element.attribute("extension")?));
                }
            }
            [
                "controlActProcess",
                "queryByParameter",
                "responsePriorityCode",
            ] => {
                self.priority = element.attribute("code")?;
            }
            [
                "controlActProcess",
                "queryByParameter",
                "matchCriterionList",
                "minimumDegreeMatch",
                "value",
            ] => {
                self.parameters.minimum_degree_match = element
                    .attribute("value")?
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
                parameter_opened(self, parameter(name), rest, element)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn closed(&mut self, open: &[(Ns, &str)], words: Option<&str>, at: usize) {
        let words = words.unwrap_or_default().trim();
        match open {
            [
                (Ns::Soap, "Envelope"),
                (Ns::Soap, "Header"),
                (Ns::Wsa, "Action"),
            ] => {
                self.action = Some(words.to_owned());
            }
            [
                (Ns::Soap, "Envelope"),
                (Ns::Soap, "Header"),
                (Ns::Wsa, "MessageID"),
            ] => {
                self.message_id = Some(words.to_owned());
            }
            [
                (Ns::Soap, "Envelope"),
                (Ns::Soap, "Header"),
                (Ns::Wsa, "ReplyTo"),
                (Ns::Wsa, "Address"),
            ] => self.reply_to = Some(words.to_owned()),
            [
                (Ns::Soap, "Envelope"),
                (Ns::Soap, "Header"),
                (_, "CorrelationTimeToLive"),
            ] => {
                self.correlation = Some(words.to_owned());
            }
            _ => {}
        }
        if !self.query {
            return;
        }
        let Some(path) = inside(open) else {
            return;
        };
        match path.as_slice() {
            ["controlActProcess", "queryByParameter"] => {
                if let Some(segment) = self.segment.as_mut() {
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
                parameter_closed(self, parameter(name), rest, words);
            }
            _ => {}
        }
    }
}

/// What an element inside the parameter `name` says as it opens.
fn parameter_opened(
    collected: &mut Collected,
    name: Option<&str>,
    rest: &[&str],
    element: &Element<'_>,
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
            parameters.gender = element.attribute("code")?;
        }
        (Some("livingSubjectBirthTime"), ["value"]) => {
            collected.birth = Some((element.attribute("value")?, None, None));
        }
        (Some("livingSubjectBirthTime"), ["value", "low"]) => {
            if let Some(birth) = collected.birth.as_mut() {
                birth.1 = element.attribute("value")?;
            }
        }
        (Some("livingSubjectBirthTime"), ["value", "high"]) => {
            if let Some(birth) = collected.birth.as_mut() {
                birth.2 = element.attribute("value")?;
            }
        }
        (Some("livingSubjectId"), ["value"]) => {
            if let Some(root) = element.attribute("root")? {
                let value = element.attribute("extension")?.unwrap_or_default();
                parameters.ids.push(PatientIdentifier::new(
                    Oid::new(&root)?,
                    SecretString::from(value),
                )?);
            }
        }
        (Some("patientTelecom"), ["value"]) => {
            if let Some(value) = element.attribute("value")? {
                parameters.telecom.push(value);
            }
        }
        (Some("principalCareProviderId"), ["value"]) => {
            if let Some(root) = element.attribute("root")? {
                parameters
                    .care_providers
                    .push((Oid::new(&root)?, element.attribute("extension")?));
            }
        }
        _ => {}
    }
    Ok(())
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
