// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The DICOM PS3.15 Annex A.5 `AuditMessage`, the XML an ITI-20 syslog
//! message carries (ITI TF-2 §3.20.4.1.2).
//!
//! The model holds what an IHE transaction's audit table fills: the event,
//! the active participants, the audit source and the participant objects. A
//! participant object's id, query and detail values may name a patient, so
//! they are held as secrets, and the written message is a secret too.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesText, Event};
use secrecy::{ExposeSecret, SecretSlice, SecretString};

use crate::redact::REDACTED;

/// A coded value, written `EV(code, codeSystemName, originalText)` in the
/// IHE audit tables: the attributes `csd-code`, `codeSystemName` and
/// `originalText` of the DICOM `CodedValueType`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodedValue {
    /// `csd-code`.
    pub code: String,
    /// `codeSystemName`.
    pub system_name: String,
    /// `originalText`.
    pub original_text: String,
}

impl CodedValue {
    /// The coded value `EV(code, system_name, original_text)`.
    #[must_use]
    pub fn new(code: &str, system_name: &str, original_text: &str) -> Self {
        Self {
            code: code.to_owned(),
            system_name: system_name.to_owned(),
            original_text: original_text.to_owned(),
        }
    }

    /// The coded value of a `(code, codeSystemName, originalText)` triple.
    #[must_use]
    pub fn of((code, system_name, original_text): (&str, &str, &str)) -> Self {
        Self::new(code, system_name, original_text)
    }
}

/// `EventIdentification`: what happened, when, and how it ended.
#[derive(Debug, Clone)]
pub struct EventIdentification {
    /// `EventID`.
    pub event_id: CodedValue,
    /// `EventActionCode`: `C`, `R`, `U`, `D` or `E`.
    pub action: &'static str,
    /// `EventDateTime`.
    pub date_time: Timestamp,
    /// `EventOutcomeIndicator`: `0`, `4`, `8` or `12`.
    pub outcome: &'static str,
    /// `EventTypeCode`, in order.
    pub type_codes: Vec<CodedValue>,
}

/// A `NetworkAccessPointID` with its `NetworkAccessPointTypeCode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessPoint {
    /// `NetworkAccessPointTypeCode`: `1` for a machine name, `2` for an IP
    /// address.
    pub type_code: &'static str,
    /// `NetworkAccessPointID`.
    pub id: String,
}

/// `ActiveParticipant`: a user, process or system that took part.
///
/// A participant may be a person, so `Debug` shows neither its `UserID` nor
/// its `UserName`.
#[derive(Clone)]
pub struct ActiveParticipant {
    /// `UserID`.
    pub user_id: String,
    /// `AlternativeUserID`, when the table fills it.
    pub alternative_user_id: Option<String>,
    /// `UserName`, when the table or a grouped profile fills it.
    pub user_name: Option<String>,
    /// `UserIsRequestor`.
    pub user_is_requestor: bool,
    /// `RoleIDCode`, in order.
    pub role_id_codes: Vec<CodedValue>,
    /// The network access point, when the participant has one.
    pub access_point: Option<AccessPoint>,
}

impl fmt::Debug for ActiveParticipant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActiveParticipant")
            .field("user_id", &REDACTED)
            .field("alternative_user_id", &self.alternative_user_id)
            .field("user_name", &self.user_name.as_ref().map(|_| REDACTED))
            .field("user_is_requestor", &self.user_is_requestor)
            .field("role_id_codes", &self.role_id_codes)
            .field("access_point", &self.access_point)
            .finish()
    }
}

/// `AuditSourceIdentification`: the system that wrote the message.
///
/// No `AuditSourceTypeCode` is written: the IHE audit tables this crate
/// fills leave it not specialized, and the PS3.15 A.5.1 schema makes it
/// optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditSource {
    /// `AuditSourceID`.
    pub id: String,
    /// `AuditEnterpriseSiteID`, when the deployment names one.
    pub enterprise_site: Option<String>,
}

/// `ParticipantObjectIdentification`: an object the event concerned.
#[derive(Clone)]
pub struct ParticipantObject {
    /// `ParticipantObjectID`.
    pub id: SecretString,
    /// `ParticipantObjectTypeCode`, when the table fills it.
    pub type_code: Option<&'static str>,
    /// `ParticipantObjectTypeCodeRole`, when the table fills it.
    pub type_code_role: Option<&'static str>,
    /// `ParticipantObjectIDTypeCode`.
    pub id_type_code: CodedValue,
    /// `ParticipantObjectQuery`, written base64-encoded.
    pub query: Option<SecretString>,
    /// `ParticipantObjectDetail` pairs: the type, and the value written
    /// base64-encoded.
    pub details: Vec<(String, SecretString)>,
}

impl fmt::Debug for ParticipantObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParticipantObject")
            .field("id", &REDACTED)
            .field("type_code", &self.type_code)
            .field("type_code_role", &self.type_code_role)
            .field("id_type_code", &self.id_type_code)
            .field("query", &self.query.as_ref().map(|_| REDACTED))
            .field(
                "details",
                &self
                    .details
                    .iter()
                    .map(|(kind, _)| (kind.as_str(), REDACTED))
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// One DICOM PS3.15 Annex A.5 `AuditMessage`.
#[derive(Debug, Clone)]
pub struct AuditMessage {
    /// `EventIdentification`.
    pub event: EventIdentification,
    /// `ActiveParticipant`, at least one, in order.
    pub participants: Vec<ActiveParticipant>,
    /// `AuditSourceIdentification`.
    pub source: AuditSource,
    /// `ParticipantObjectIdentification`, in order.
    pub objects: Vec<ParticipantObject>,
}

/// Why a message could not be written.
#[derive(Debug, thiserror::Error)]
#[error("the audit message could not be written")]
pub struct MessageError(#[source] std::io::Error);

impl AuditMessage {
    /// The message as UTF-8 XML, with no byte order mark (ITI TF-2
    /// §3.20.4.1.2), in the element order of the PS3.15 A.5.1 schema.
    ///
    /// # Errors
    ///
    /// A [`MessageError`] when the writer fails, which writing to memory does
    /// not.
    pub fn to_xml(&self) -> Result<SecretSlice<u8>, MessageError> {
        let mut writer = Writer::new(Vec::new());
        writer
            .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
            .map_err(MessageError)?;
        writer
            .create_element("AuditMessage")
            .write_inner_content(|writer| {
                self.write_event(writer)?;
                for participant in &self.participants {
                    write_participant(writer, participant)?;
                }
                self.write_source(writer)?;
                for object in &self.objects {
                    write_object(writer, object)?;
                }
                Ok(())
            })
            .map_err(MessageError)?;
        Ok(SecretSlice::from(writer.into_inner()))
    }

    fn write_event(&self, writer: &mut Writer<Vec<u8>>) -> std::io::Result<()> {
        let event = &self.event;
        let date_time = event.date_time.to_string();
        writer
            .create_element("EventIdentification")
            .with_attributes([
                ("EventActionCode", event.action),
                ("EventDateTime", date_time.as_str()),
                ("EventOutcomeIndicator", event.outcome),
            ])
            .write_inner_content(|writer| {
                write_coded(writer, "EventID", &event.event_id)?;
                for code in &event.type_codes {
                    write_coded(writer, "EventTypeCode", code)?;
                }
                Ok(())
            })?;
        Ok(())
    }

    fn write_source(&self, writer: &mut Writer<Vec<u8>>) -> std::io::Result<()> {
        let source = &self.source;
        let mut attributes = Vec::new();
        if let Some(site) = &source.enterprise_site {
            attributes.push(("AuditEnterpriseSiteID", site.as_str()));
        }
        attributes.push(("AuditSourceID", source.id.as_str()));
        writer
            .create_element("AuditSourceIdentification")
            .with_attributes(attributes)
            .write_empty()?;
        Ok(())
    }
}

fn write_coded(
    writer: &mut Writer<Vec<u8>>,
    element: &str,
    code: &CodedValue,
) -> std::io::Result<()> {
    writer
        .create_element(element)
        .with_attributes([
            ("csd-code", code.code.as_str()),
            ("codeSystemName", code.system_name.as_str()),
            ("originalText", code.original_text.as_str()),
        ])
        .write_empty()?;
    Ok(())
}

fn write_participant(
    writer: &mut Writer<Vec<u8>>,
    participant: &ActiveParticipant,
) -> std::io::Result<()> {
    let mut attributes = vec![("UserID", participant.user_id.as_str())];
    if let Some(alternative) = &participant.alternative_user_id {
        attributes.push(("AlternativeUserID", alternative.as_str()));
    }
    if let Some(name) = &participant.user_name {
        attributes.push(("UserName", name.as_str()));
    }
    attributes.push((
        "UserIsRequestor",
        if participant.user_is_requestor {
            "true"
        } else {
            "false"
        },
    ));
    if let Some(point) = &participant.access_point {
        attributes.push(("NetworkAccessPointID", point.id.as_str()));
        attributes.push(("NetworkAccessPointTypeCode", point.type_code));
    }
    writer
        .create_element("ActiveParticipant")
        .with_attributes(attributes)
        .write_inner_content(|writer| {
            for code in &participant.role_id_codes {
                write_coded(writer, "RoleIDCode", code)?;
            }
            Ok(())
        })?;
    Ok(())
}

fn write_object(writer: &mut Writer<Vec<u8>>, object: &ParticipantObject) -> std::io::Result<()> {
    let mut attributes = vec![("ParticipantObjectID", object.id.expose_secret())];
    if let Some(code) = object.type_code {
        attributes.push(("ParticipantObjectTypeCode", code));
    }
    if let Some(role) = object.type_code_role {
        attributes.push(("ParticipantObjectTypeCodeRole", role));
    }
    let details: Vec<(&str, String)> = object
        .details
        .iter()
        .map(|(kind, value)| (kind.as_str(), STANDARD.encode(value.expose_secret())))
        .collect();
    writer
        .create_element("ParticipantObjectIdentification")
        .with_attributes(attributes)
        .write_inner_content(|writer| {
            write_coded(writer, "ParticipantObjectIDTypeCode", &object.id_type_code)?;
            if let Some(query) = &object.query {
                writer
                    .create_element("ParticipantObjectQuery")
                    .write_text_content(BytesText::new(&STANDARD.encode(query.expose_secret())))?;
            }
            for (kind, value) in &details {
                writer
                    .create_element("ParticipantObjectDetail")
                    .with_attributes([("type", *kind), ("value", value.as_str())])
                    .write_empty()?;
            }
            Ok(())
        })?;
    Ok(())
}
