// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Writing the Responding Gateway's answers to a Cross Gateway Patient
//! Discovery Request.
//!
//! The answer is a `PRPA_IN201306UV02` in a SOAP 1.2 envelope with its
//! WS-Addressing headers, for the five cases of ITI TF-2 §3.55.4.2.3. A
//! Deferred request the gateway does not take is refused with the
//! `MCCI_IN000002UV01` acknowledgement of §3.55.4.1.3, and a request that is
//! no ITI-55 query with a SOAP 1.2 fault.

use std::fmt;

use jiff::Timestamp;
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesText, Event};
use secrecy::{ExposeSecret as _, SecretString};
use uuid::Uuid;

use super::super::discovery::RequestedAttribute;
use super::super::error::DetectedIssue;
use super::super::identifier::{HomeCommunityId, Oid};
use super::super::{HL7, SOAP, WSA};
use super::query::{PersonName, PostalAddress, ReceivedQuery};
use ihe_atna::redact::REDACTED;

mod subject;

use subject::{detected_issue, registration};

/// The WS-Addressing action of the response (§3.55.6.1.1).
pub const RESPONSE_ACTION: &str = "urn:hl7-org:v3:PRPA_IN201306UV02:CrossGatewayPatientDiscovery";

/// The WS-Addressing action of an acknowledgement (§3.55.6.1.1).
const ACKNOWLEDGEMENT_ACTION: &str = "urn:hl7-org:v3:MCCI_IN000002UV01";

/// The OID of the HL7 interaction and trigger event identifiers.
const HL7_INTERACTION: &str = "2.16.840.1.113883.1.6";

/// The code system of the Health Data Locator designation
/// (Table 3.55.4.2.2.5-1).
const LOCATOR_SYSTEM: &str = "1.3.6.1.4.1.19376.1.2.27.2";

/// The HL7 v3 code system of the detected issue event's code
/// (§3.55.4.2.2.6).
const ACT_CODE: &str = "2.16.840.1.113883.5.4";

/// The SOAP 1.2 media type of an answer.
pub const MEDIA_TYPE: &str = "application/soap+xml; charset=UTF-8";

/// One matching patient record: one `RegistrationEvent` of the answer
/// (§3.55.4.2.2.2).
///
/// `Debug` shows the community and how many identifiers and names the
/// record holds, never a value.
#[derive(Clone)]
pub struct Candidate {
    /// The community that holds the record, the custodian's
    /// `homeCommunityId` (§3.55.4.2.2.4).
    pub community: HomeCommunityId,
    /// `Patient.id`: the identifier a Cross Gateway Query to this gateway
    /// uses, its assigning authority and value (Table 3.55.4.2.2.2-1).
    pub patient_id: (Oid, SecretString),
    /// `OtherIDs.id`: every other identifier of the patient, each with its
    /// assigning authority, which also scopes it.
    pub other_ids: Vec<(Oid, SecretString)>,
    /// `Person.name`; empty writes the `NA` null flavor, which the table
    /// admits "only if the request contained only a patient identifier".
    pub names: Vec<PersonName>,
    /// `Person.administrativeGenderCode`, an HL7 v3 code.
    pub gender: Option<String>,
    /// `Person.birthTime`, an HL7 v3 `TS` text.
    pub birth_time: Option<String>,
    /// `Person.addr`.
    pub addresses: Vec<PostalAddress>,
}

impl fmt::Debug for Candidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Candidate")
            .field("community", &self.community)
            .field("patient_id", &(&self.patient_id.0, REDACTED))
            .field("other_ids", &self.other_ids.len())
            .field("names", &self.names.len())
            .finish_non_exhaustive()
    }
}

/// What the Responding Gateway answers, one of the five cases of
/// §3.55.4.2.3.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Cases 1 and 2: one or more matching records.
    Matched(Vec<Candidate>),
    /// Case 3: candidates that do not match closely enough, and the
    /// attributes that might allow a match (§3.55.4.2.2.6).
    MoreAttributes(Vec<RequestedAttribute>),
    /// Case 4: no patient anywhere close to matching.
    NoMatch,
    /// Case 5: the gateway cannot satisfy the request, with the problems it
    /// codes (§3.55.4.2.2.7).
    Unable(Vec<DetectedIssue>),
}

impl Outcome {
    /// The `Acknowledgement.typeCode` and `QueryAck.queryResponseCode` the
    /// case writes.
    fn codes(&self) -> (&'static str, &'static str) {
        match self {
            Self::Matched(_) | Self::MoreAttributes(_) => ("AA", "OK"),
            Self::NoMatch => ("AA", "NF"),
            Self::Unable(_) => ("AE", "AE"),
        }
    }
}

/// The Responding Gateway, as its answers name it: its device, the one
/// sender of every answer.
#[derive(Debug, Clone)]
pub struct Responder {
    /// The gateway's device.
    pub device: Oid,
    /// The gateway's community, the sender device's organization.
    pub community: HomeCommunityId,
}

type Out = Writer<Vec<u8>>;

/// The answer to `query` from `responder`: the SOAP 1.2 envelope of the
/// `PRPA_IN201306UV02` that `outcome` writes (§3.55.4.2.2), with the
/// `RelatesTo` header naming the request.
///
/// # Errors
///
/// The writer's error when the answer cannot be written.
pub fn envelope(
    query: &ReceivedQuery,
    responder: &Responder,
    outcome: &Outcome,
) -> std::io::Result<Vec<u8>> {
    let message = Uuid::new_v4();
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    writer
        .create_element("soap:Envelope")
        .with_attributes([("xmlns:soap", SOAP), ("xmlns:wsa", WSA)])
        .write_inner_content(|writer| {
            writer
                .create_element("soap:Header")
                .write_inner_content(|writer| {
                    addressing(writer, RESPONSE_ACTION, message, query)
                })?;
            writer
                .create_element("soap:Body")
                .write_inner_content(|writer| {
                    response(writer, query, responder, outcome, message)
                })?;
            Ok(())
        })?;
    Ok(writer.into_inner())
}

/// The `MCCI_IN000002UV01` acknowledgement of a Deferred request the gateway
/// does not take: `AE` with the detail `NS250`, "Unsupported processing
/// mode" (§3.55.4.1.3).
///
/// # Errors
///
/// The writer's error when the acknowledgement cannot be written.
pub fn deferred_refused(query: &ReceivedQuery, responder: &Responder) -> std::io::Result<Vec<u8>> {
    let message = Uuid::new_v4();
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    writer
        .create_element("soap:Envelope")
        .with_attributes([("xmlns:soap", SOAP), ("xmlns:wsa", WSA)])
        .write_inner_content(|writer| {
            writer
                .create_element("soap:Header")
                .write_inner_content(|writer| {
                    addressing(writer, ACKNOWLEDGEMENT_ACTION, message, query)
                })?;
            writer
                .create_element("soap:Body")
                .write_inner_content(|writer| {
                    writer
                        .create_element("MCCI_IN000002UV01")
                        .with_attributes([("xmlns", HL7), ("ITSVersion", "XML_1.0")])
                        .write_inner_content(|writer| {
                            wrapper(writer, query, responder, message, "MCCI_IN000002UV01")?;
                            acknowledgement(writer, query, "AE", |writer| {
                                writer
                                    .create_element("acknowledgementDetail")
                                    .with_attribute(("typeCode", "E"))
                                    .write_inner_content(|writer| {
                                        writer
                                            .create_element("code")
                                            .with_attributes([
                                                ("code", "NS250"),
                                                ("displayName", "Unsupported processing mode"),
                                            ])
                                            .write_empty()?;
                                        writer.create_element("text").write_text_content(
                                            BytesText::new("Deferred Response not supported."),
                                        )?;
                                        Ok(())
                                    })?;
                                Ok(())
                            })?;
                            Ok(())
                        })?;
                    Ok(())
                })?;
            Ok(())
        })?;
    Ok(writer.into_inner())
}

/// Who a SOAP fault blames (SOAP 1.2 Part 1 §5.4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blame {
    /// `Sender`: the request is at fault.
    Sender,
    /// `Receiver`: the gateway is at fault.
    Receiver,
}

/// The SOAP 1.2 fault of a request the gateway cannot read as an ITI-55
/// query, a transmission error (§3.55.4.2.3), with `reason` as its text.
///
/// `reason` is the gateway's own words; it never quotes the request.
///
/// # Errors
///
/// The writer's error when the fault cannot be written.
pub fn fault(blame: Blame, reason: &str) -> std::io::Result<Vec<u8>> {
    let code = match blame {
        Blame::Sender => "soap:Sender",
        Blame::Receiver => "soap:Receiver",
    };
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    writer
        .create_element("soap:Envelope")
        .with_attribute(("xmlns:soap", SOAP))
        .write_inner_content(|writer| {
            writer
                .create_element("soap:Body")
                .write_inner_content(|writer| {
                    writer
                        .create_element("soap:Fault")
                        .write_inner_content(|writer| {
                            writer
                                .create_element("soap:Code")
                                .write_inner_content(|writer| {
                                    writer
                                        .create_element("soap:Value")
                                        .write_text_content(BytesText::new(code))?;
                                    Ok(())
                                })?;
                            writer
                                .create_element("soap:Reason")
                                .write_inner_content(|writer| {
                                    writer
                                        .create_element("soap:Text")
                                        .with_attribute(("xml:lang", "en"))
                                        .write_text_content(BytesText::new(reason))?;
                                    Ok(())
                                })?;
                            Ok(())
                        })?;
                    Ok(())
                })?;
            Ok(())
        })?;
    Ok(writer.into_inner())
}

/// The WS-Addressing headers of an answer: its action, its own
/// `MessageID`, the `RelatesTo` of the request and, for an asynchronous
/// exchange, the `To` of its reply endpoint (Appendix V).
fn addressing(
    writer: &mut Out,
    action: &str,
    message: Uuid,
    query: &ReceivedQuery,
) -> std::io::Result<()> {
    // NOTE: ITI TF-2 V.3.2.2 IHE-WSA101: every wsa:Action carries mustUnderstand.
    writer
        .create_element("wsa:Action")
        .with_attribute(("soap:mustUnderstand", "1"))
        .write_text_content(BytesText::new(action))?;
    writer
        .create_element("wsa:MessageID")
        .write_text_content(BytesText::new(&format!("urn:uuid:{message}")))?;
    writer
        .create_element("wsa:RelatesTo")
        .write_text_content(BytesText::new(&query.message_id))?;
    // NOTE: ITI TF-2 V.9.2.4: an asynchronous response names the request's ReplyTo as wsa:To.
    if let Some(reply_to) = &query.reply_to {
        writer
            .create_element("wsa:To")
            .with_attribute(("soap:mustUnderstand", "1"))
            .write_text_content(BytesText::new(reply_to))?;
    }
    Ok(())
}

/// The `PRPA_IN201306UV02` interaction: the Application Acknowledgement
/// transmission wrapper (Appendix O.1.3) and the Registry Query Response
/// Control Act (Table 3.55.4.2.2.3-1).
fn response(
    writer: &mut Out,
    query: &ReceivedQuery,
    responder: &Responder,
    outcome: &Outcome,
    message: Uuid,
) -> std::io::Result<()> {
    let (acknowledged, answered) = outcome.codes();
    writer
        .create_element("PRPA_IN201306UV02")
        .with_attributes([("xmlns", HL7), ("ITSVersion", "XML_1.0")])
        .write_inner_content(|writer| {
            wrapper(writer, query, responder, message, "PRPA_IN201306UV02")?;
            acknowledgement(writer, query, acknowledged, |_writer| Ok(()))?;
            writer
                .create_element("controlActProcess")
                .with_attributes([("classCode", "CACT"), ("moodCode", "EVN")])
                .write_inner_content(|writer| {
                    writer
                        .create_element("code")
                        .with_attributes([
                            ("code", "PRPA_TE201306UV02"),
                            ("codeSystem", HL7_INTERACTION),
                        ])
                        .write_empty()?;
                    if let Outcome::Matched(candidates) = outcome {
                        for candidate in candidates {
                            registration(writer, candidate)?;
                        }
                    }
                    detected_issue(writer, outcome)?;
                    query_ack(writer, query, answered)?;
                    // NOTE: the segment is the request's own well-formed element, read
                    // whole, and Table 3.55.4.2.2.3-1 asks for its copy after the queryAck.
                    writer.write_event(Event::Text(BytesText::from_escaped(
                        query.query_by_parameter.expose_secret(),
                    )))?;
                    Ok(())
                })?;
            Ok(())
        })?;
    Ok(())
}

/// The transmission wrapper's `id`, `creationTime`, `interactionId`, codes,
/// and its receiver (the initiating gateway's device) and sender (this
/// gateway's device).
fn wrapper(
    writer: &mut Out,
    query: &ReceivedQuery,
    responder: &Responder,
    message: Uuid,
    interaction: &str,
) -> std::io::Result<()> {
    let id = message.to_string();
    let created = Timestamp::now().strftime("%Y%m%d%H%M%S+0000").to_string();
    writer
        .create_element("id")
        .with_attribute(("root", id.as_str()))
        .write_empty()?;
    writer
        .create_element("creationTime")
        .with_attribute(("value", created.as_str()))
        .write_empty()?;
    writer
        .create_element("interactionId")
        .with_attributes([("root", HL7_INTERACTION), ("extension", interaction)])
        .write_empty()?;
    code(writer, "processingCode", processing(query))?;
    code(writer, "processingModeCode", "T")?;
    code(writer, "acceptAckCode", "NE")?;
    let initiating = query
        .sender_device
        .as_ref()
        .map_or("", |device| device.as_str());
    device(
        writer,
        ("receiver", "RCV"),
        initiating,
        query.sender_community.as_ref(),
    )?;
    device(
        writer,
        ("sender", "SND"),
        responder.device.as_str(),
        Some(&responder.community),
    )?;
    Ok(())
}

/// The request's `processingCode`, which the answer keeps.
fn processing(query: &ReceivedQuery) -> &'static str {
    use super::super::request::ProcessingCode;
    match query.processing {
        ProcessingCode::Production => "P",
        ProcessingCode::Training => "T",
        ProcessingCode::Debugging => "D",
    }
}

/// The `acknowledgement` with `type_code`, naming the request's
/// transmission `id` as its target message, and what `detail` writes
/// inside it.
fn acknowledgement(
    writer: &mut Out,
    query: &ReceivedQuery,
    type_code: &str,
    detail: impl FnOnce(&mut Out) -> std::io::Result<()>,
) -> std::io::Result<()> {
    writer
        .create_element("acknowledgement")
        .write_inner_content(|writer| {
            code(writer, "typeCode", type_code)?;
            writer
                .create_element("targetMessage")
                .write_inner_content(|writer| {
                    ii(
                        writer,
                        "id",
                        &query.transmission.0,
                        query.transmission.1.as_deref(),
                    )
                })?;
            detail(writer)
        })?;
    Ok(())
}

/// The `queryAck` naming the request's `queryId`, with `status` as its
/// response code; the quantities are not populated
/// (Table 3.55.4.2.2.3-1).
fn query_ack(writer: &mut Out, query: &ReceivedQuery, status: &str) -> std::io::Result<()> {
    writer
        .create_element("queryAck")
        .write_inner_content(|writer| {
            ii(
                writer,
                "queryId",
                &query.query_id.0,
                query.query_id.1.as_deref(),
            )?;
            code(writer, "queryResponseCode", status)?;
            Ok(())
        })?;
    Ok(())
}

/// An `II` element `name` with `root` and, when given, `extension`.
fn ii(writer: &mut Out, name: &str, root: &str, extension: Option<&str>) -> std::io::Result<()> {
    let element = writer.create_element(name).with_attribute(("root", root));
    match extension {
        Some(extension) => element
            .with_attribute(("extension", extension))
            .write_empty()?,
        None => element.write_empty()?,
    };
    Ok(())
}

/// An element with one `code` attribute.
fn code(writer: &mut Out, name: &str, value: &str) -> std::io::Result<()> {
    writer
        .create_element(name)
        .with_attribute(("code", value))
        .write_empty()?;
    Ok(())
}

/// A `receiver` or `sender` with its device `id`, and the device's
/// organization when a community is named.
fn device(
    writer: &mut Out,
    (role, type_code): (&str, &str),
    device: &str,
    community: Option<&HomeCommunityId>,
) -> std::io::Result<()> {
    writer
        .create_element(role)
        .with_attribute(("typeCode", type_code))
        .write_inner_content(|writer| {
            writer
                .create_element("device")
                .with_attributes([("classCode", "DEV"), ("determinerCode", "INSTANCE")])
                .write_inner_content(|writer| {
                    if device.is_empty() {
                        writer
                            .create_element("id")
                            .with_attribute(("nullFlavor", "NI"))
                            .write_empty()?;
                    } else {
                        ii(writer, "id", device, None)?;
                    }
                    if let Some(community) = community {
                        writer
                            .create_element("asAgent")
                            .with_attribute(("classCode", "AGNT"))
                            .write_inner_content(|writer| {
                                writer
                                    .create_element("representedOrganization")
                                    .with_attributes([
                                        ("classCode", "ORG"),
                                        ("determinerCode", "INSTANCE"),
                                    ])
                                    .write_inner_content(|writer| {
                                        ii(writer, "id", community.oid().as_str(), None)
                                    })?;
                                Ok(())
                            })?;
                    }
                    Ok(())
                })?;
            Ok(())
        })?;
    Ok(())
}
