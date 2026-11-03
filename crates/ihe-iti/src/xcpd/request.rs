// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Cross Gateway Patient Discovery Request: a `PRPA_IN201305UV02` in a
//! SOAP 1.2 envelope with its WS-Addressing headers (ITI TF-2 §3.55.4.1,
//! §3.55.6, Appendix O and Appendix V).

use std::fmt;

use jiff::Timestamp;
use quick_xml::Writer;
use quick_xml::events::{BytesText, Event};
use secrecy::{ExposeSecret as _, SecretString};
use url::Url;
use uuid::Uuid;

use super::error::{InvalidInput, XcpdError};
use super::identifier::{HomeCommunityId, Oid, PatientIdentifier};
use super::security::XuaAssertion;
use super::v3::{self, Out};
use super::{ACTION, HL7, WSSE};
use crate::soap::{self, ANONYMOUS};
use ihe_atna::redact::RedactedUrl;

/// Whether a message belongs to production, training or debugging
/// (`processingCode`, Appendix O Table O.1.1-1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProcessingCode {
    /// `P`, a production message.
    #[default]
    Production,
    /// `T`, a training message.
    Training,
    /// `D`, a debugging message.
    Debugging,
}

impl ProcessingCode {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::Production => "P",
            Self::Training => "T",
            Self::Debugging => "D",
        }
    }
}

/// A Responding Gateway, as the initiating gateway addresses it.
///
/// It is the SOAP endpoint a request is posted to, the device the
/// transmission wrapper names as its one receiver, and the community to
/// search when the gateway serves several (§3.55.4.1.2.5).
#[derive(Clone)]
pub struct RespondingGateway {
    endpoint: Url,
    device: Oid,
    community: Option<HomeCommunityId>,
}

impl RespondingGateway {
    /// The gateway at the `https` URL `endpoint`, whose receiver device is
    /// `device`.
    ///
    /// The request carries the patient identifier and any XUA assertion, a
    /// bearer credential, so it travels over TLS only.
    ///
    /// # Errors
    /// [`InvalidInput::Endpoint`] when `endpoint` is not an `https` URL
    /// without a fragment.
    pub fn new(endpoint: Url, device: Oid) -> Result<Self, InvalidInput> {
        // NOTE: ITI TF-1 Table 27.1.3-1, §27.4.1: an XCPD actor is an ATNA Secure Node
        // or Secure Application, so "outgoing messages will be via a secure communication channel".
        if endpoint.scheme() != "https" {
            return Err(InvalidInput::Endpoint);
        }
        Self::unencrypted_for_development(endpoint, device)
    }

    /// The gateway at `endpoint`, `http` or `https`, for development and
    /// tests only.
    ///
    /// Over `http` the patient identifier and any XUA assertion cross the
    /// network in clear text, which no XCPD deployment permits (ITI TF-1
    /// §27.4.1). A caller offers this only where its own configuration is
    /// marked for development.
    ///
    /// # Errors
    /// [`InvalidInput::Endpoint`] when `endpoint` is not an `http` or `https`
    /// URL without a fragment.
    pub fn unencrypted_for_development(endpoint: Url, device: Oid) -> Result<Self, InvalidInput> {
        if !matches!(endpoint.scheme(), "http" | "https") || endpoint.fragment().is_some() {
            return Err(InvalidInput::Endpoint);
        }
        Ok(Self {
            endpoint,
            device,
            community: None,
        })
    }

    /// This gateway, asked for the results of `community` only
    /// (§3.55.4.1.2.5). The responding gateway SHOULD honour it, and the
    /// answer is read for every community it names either way
    /// (§3.55.4.2.2.4).
    #[must_use]
    pub fn targeting(mut self, community: HomeCommunityId) -> Self {
        self.community = Some(community);
        self
    }

    /// The SOAP endpoint.
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// The receiver device.
    #[must_use]
    pub fn device(&self) -> &Oid {
        &self.device
    }
}

impl fmt::Debug for RespondingGateway {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RespondingGateway")
            .field("endpoint", &RedactedUrl(self.endpoint.as_str()))
            .field("device", &self.device)
            .field("community", &self.community)
            .finish()
    }
}

/// A discovery by the shared patient identifier.
///
/// It is the "Shared/national Patient Identifier Query and Feed" mode of
/// §3.55.1: the identifier alone, no demographics, since matching "can be
/// done on the identifier alone".
#[derive(Debug, Clone)]
pub struct DiscoveryQuery {
    sender: Oid,
    community: Option<HomeCommunityId>,
    patient: PatientIdentifier,
    processing: ProcessingCode,
}

impl DiscoveryQuery {
    /// A discovery of `patient`, sent by the device `sender`.
    #[must_use]
    pub fn new(sender: Oid, patient: PatientIdentifier) -> Self {
        Self {
            sender,
            community: None,
            patient,
            processing: ProcessingCode::default(),
        }
    }

    /// This query, naming the initiating community `community` as the
    /// sender's organization (§3.55.4.1.2.4). An initiating gateway grouped
    /// with a responding gateway shall name it; one that is not need not.
    #[must_use]
    pub fn sent_for(mut self, community: HomeCommunityId) -> Self {
        self.community = Some(community);
        self
    }

    /// This query, sent under `processing` instead of production.
    #[must_use]
    pub fn processing(mut self, processing: ProcessingCode) -> Self {
        self.processing = processing;
        self
    }

    /// The patient identifier asked about.
    #[must_use]
    pub fn patient(&self) -> &PatientIdentifier {
        &self.patient
    }

    /// The initiating community the query names, if any.
    #[must_use]
    pub fn community(&self) -> Option<&HomeCommunityId> {
        self.community.as_ref()
    }
}

/// The identifiers one request carries: the WS-Addressing `MessageID` and
/// the transmission wrapper's `id` share one UUID, the query its own.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ids {
    pub(super) message: Uuid,
    pub(super) query: Uuid,
    pub(super) created: Timestamp,
}

impl Ids {
    /// Fresh identifiers, created now.
    pub(super) fn fresh() -> Self {
        Self {
            message: Uuid::new_v4(),
            query: Uuid::new_v4(),
            created: Timestamp::now(),
        }
    }

    /// The `MessageID` the answer's `RelatesTo` names.
    pub(super) fn message_urn(&self) -> String {
        format!("urn:uuid:{}", self.message)
    }
}

/// The SOAP 1.2 envelope of `query` to `gateway` (§3.55.4.1.2, §3.55.6.1,
/// Appendix V.3.2.2), with `assertion` in a WS-Security header when one is
/// given.
pub(super) fn envelope(
    query: &DiscoveryQuery,
    gateway: &RespondingGateway,
    ids: &Ids,
    assertion: Option<&XuaAssertion>,
) -> Result<Written, XcpdError> {
    let segment = query_by_parameter(query, ids).map_err(XcpdError::Encode)?;
    let body = soap::write(
        |writer| header(writer, gateway, ids, assertion),
        |writer| message(writer, query, gateway, ids, &segment),
    )
    .map_err(XcpdError::Encode)?;
    Ok(Written {
        body,
        query: SecretString::from(segment),
    })
}

/// A request as written: the envelope, and the `queryByParameter` segment
/// it carries, which the audit message records (§3.55.5.1.1).
pub(super) struct Written {
    /// The SOAP envelope.
    pub(super) body: Vec<u8>,
    /// The `queryByParameter` segment, which names the patient identifier.
    pub(super) query: SecretString,
}

/// The WS-Addressing headers Appendix V.3.2.2 requires, and the XUA
/// assertion.
fn header(
    writer: &mut Out,
    gateway: &RespondingGateway,
    ids: &Ids,
    assertion: Option<&XuaAssertion>,
) -> std::io::Result<()> {
    // NOTE: ITI TF-2 V.3.2.2 IHE-WSA101: every wsa:Action carries mustUnderstand.
    writer
        .create_element("wsa:Action")
        .with_attribute(("soap:mustUnderstand", "1"))
        .write_text_content(BytesText::new(ACTION))?;
    writer
        .create_element("wsa:MessageID")
        .write_text_content(BytesText::new(&ids.message_urn()))?;
    // NOTE: ITI TF-2 V.3.2.2 IHE-WSA102: the initiating message carries wsa:ReplyTo.
    writer
        .create_element("wsa:ReplyTo")
        .write_inner_content(|writer| {
            writer
                .create_element("wsa:Address")
                .write_text_content(BytesText::new(ANONYMOUS))?;
            Ok(())
        })?;
    writer
        .create_element("wsa:To")
        .write_text_content(BytesText::new(gateway.endpoint.as_str()))?;
    if let Some(assertion) = assertion {
        writer
            .create_element("wsse:Security")
            .with_attributes([("xmlns:wsse", WSSE), ("soap:mustUnderstand", "1")])
            .write_inner_content(|writer| {
                writer.write_event(Event::Text(BytesText::from_escaped(assertion.as_str())))
            })?;
    }
    Ok(())
}

/// The `PRPA_IN201305UV02` interaction: the Send Message Payload wrapper
/// (Appendix O.1.1), the Query Control Act wrapper (Appendix O.2.3), and the
/// query by parameter (§3.55.4.1.2.2) with the constraints of
/// Table 3.55.4.1.2.3-1.
fn message(
    writer: &mut Out,
    query: &DiscoveryQuery,
    gateway: &RespondingGateway,
    ids: &Ids,
    segment: &str,
) -> std::io::Result<()> {
    writer
        .create_element("PRPA_IN201305UV02")
        .with_attributes([("xmlns", HL7), ("ITSVersion", "XML_1.0")])
        .write_inner_content(|writer| {
            v3::transmission(
                writer,
                (ids.message, ids.created),
                "PRPA_IN201305UV02",
                query.processing.code(),
                "AL",
            )?;
            v3::device(
                writer,
                ("receiver", "RCV"),
                Some(gateway.device.as_str()),
                gateway.community.as_ref(),
            )?;
            v3::device(
                writer,
                ("sender", "SND"),
                Some(query.sender.as_str()),
                query.community.as_ref(),
            )?;
            control_act(writer, segment)?;
            Ok(())
        })?;
    Ok(())
}

/// The Query Control Act: `controlActProcess` in mood `EVN` with the trigger
/// event `PRPA_TE201305UV02`, around the query by parameter `segment`.
fn control_act(writer: &mut Out, segment: &str) -> std::io::Result<()> {
    writer
        .create_element("controlActProcess")
        .with_attributes([("classCode", "CACT"), ("moodCode", "EVN")])
        .write_inner_content(|writer| {
            writer
                .create_element("code")
                .with_attributes([
                    ("code", "PRPA_TE201305UV02"),
                    ("codeSystem", v3::INTERACTION),
                ])
                .write_empty()?;
            // NOTE: the segment is this module's own writer output, well formed and
            // escaped, so it is spliced as written and the audit records the same bytes.
            writer.write_event(Event::Text(BytesText::from_escaped(segment)))?;
            Ok(())
        })?;
    Ok(())
}

/// The `queryByParameter` of `query` (§3.55.4.1.2.2): the query parameters
/// the request sends and its audit message records (§3.55.5.1.1).
fn query_by_parameter(query: &DiscoveryQuery, ids: &Ids) -> std::io::Result<String> {
    let query_id = ids.query.to_string();
    let patient = &query.patient;
    let mut out = Writer::new(Vec::new());
    {
        let writer = &mut out;
        writer
            .create_element("queryByParameter")
            .write_inner_content(|writer| {
                writer
                    .create_element("queryId")
                    .with_attribute(("root", query_id.as_str()))
                    .write_empty()?;
                v3::code(writer, "statusCode", "new")?;
                v3::code(writer, "responseModalityCode", "R")?;
                v3::code(writer, "responsePriorityCode", "I")?;
                writer
                    .create_element("parameterList")
                    .write_inner_content(|writer| {
                        // NOTE: HL7 v3 XML ITS: the element is lowerCamel, as every other
                        // parameter of the §3.55.4.1.2.4 example is.
                        writer
                            .create_element("livingSubjectId")
                            .write_inner_content(|writer| {
                                writer
                                    .create_element("value")
                                    .with_attributes([
                                        ("root", patient.authority().as_str()),
                                        ("extension", patient.value().expose_secret()),
                                    ])
                                    .write_empty()?;
                                writer
                                    .create_element("semanticsText")
                                    .write_text_content(BytesText::new("LivingSubject.id"))?;
                                Ok(())
                            })?;
                        Ok(())
                    })?;
                Ok(())
            })?;
    }
    String::from_utf8(out.into_inner()).map_err(std::io::Error::other)
}
