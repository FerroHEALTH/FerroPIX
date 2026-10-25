// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-55 Responding Gateway audit message (ITI TF-2 §3.55.5.1.2).
//!
//! A gateway that answered a query fills one [`RespondingAuditEvent`] and
//! routes it to its audit repository: the event (a DICOM `Query`, action
//! `E`, the `ITI-55` event type), the outcome, the initiating gateway as the
//! source (its `ReplyTo` for an asynchronous exchange), this gateway's SOAP
//! endpoint and process as the destination, one patient for each patient
//! whose demographics the answer returned, and the query parameters with the
//! `homeCommunityID` detail. `Debug` redacts the query and the patients.

use std::fmt;

use jiff::Timestamp;
use secrecy::SecretString;
use url::Url;

use super::super::audit::{EventOutcome, NetworkAccessPoint};
use super::super::identifier::HomeCommunityId;
use ihe_atna::redact::{REDACTED, RedactedUrl};

/// One ITI-55 Responding Gateway audit message (§3.55.5.1.2).
#[derive(Clone)]
pub struct RespondingAuditEvent {
    /// `EventDateTime`: when the answer was written.
    pub date_time: Timestamp,
    /// `EventOutcomeIndicator`.
    pub outcome: EventOutcome,
    /// The source's `UserID`: the request's `wsa:ReplyTo` for an
    /// asynchronous exchange, the anonymous address otherwise.
    pub source_user: String,
    /// The source's `NetworkAccessPointID` and type: the initiating
    /// gateway's host, as the connection names it.
    pub source_access_point: Option<NetworkAccessPoint>,
    /// The destination's `UserID`: this gateway's SOAP endpoint URI.
    pub destination: Url,
    /// The destination's `AlternativeUserID`: this process's id.
    pub process_id: u32,
    /// The `ParticipantObjectID` of each patient whose demographics the
    /// answer returned, in HL7 CX format.
    pub patients: Vec<SecretString>,
    /// The `ParticipantObjectQuery`: the request's `QueryByParameter`
    /// segment, which the DICOM encoding base64-encodes.
    pub query: SecretString,
    /// The `ParticipantObjectDetail` of type `ihe:homeCommunityID`: the
    /// initiating community the request named, when it named one.
    pub home_community: Option<HomeCommunityId>,
}

impl fmt::Debug for RespondingAuditEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RespondingAuditEvent")
            .field("date_time", &self.date_time)
            .field("outcome", &self.outcome)
            .field("source_user", &RedactedUrl(&self.source_user))
            .field("destination", &RedactedUrl(self.destination.as_str()))
            .field("process_id", &self.process_id)
            .field("patients", &self.patients.len())
            .field("query", &REDACTED)
            .field("home_community", &self.home_community)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "atna")]
impl RespondingAuditEvent {
    /// The DICOM PS3.15 audit message of this event, as the Responding
    /// Gateway's audit table fills it (§3.55.5.1.2), written by `source` on
    /// the host `host`.
    #[must_use]
    pub fn message(
        &self,
        source: &ihe_atna::atna::message::AuditSource,
        host: &ihe_atna::atna::message::AccessPoint,
    ) -> ihe_atna::atna::message::AuditMessage {
        use ihe_atna::atna::codes::OBJECT_ID_PATIENT_NUMBER;
        use ihe_atna::atna::message::{
            AccessPoint, ActiveParticipant, AuditMessage, CodedValue, EventIdentification,
            ParticipantObject,
        };

        use super::super::audit::{
            DESTINATION_ROLE, EVENT_ACTION, EVENT_ID, EVENT_TYPE, HOME_COMMUNITY_DETAIL,
            SOURCE_ROLE,
        };
        let transaction = CodedValue::of(EVENT_TYPE);
        let participants = vec![
            ActiveParticipant {
                user_id: self.source_user.clone(),
                alternative_user_id: None,
                user_name: None,
                // NOTE: DICOM PS3.15 A.5.2 names the initiator as UserIsRequestor, and
                // §3.55.5.1.2 leaves the source's flag unspecialized: the initiating gateway.
                user_is_requestor: true,
                role_id_codes: vec![CodedValue::of(SOURCE_ROLE)],
                access_point: self.source_access_point.as_ref().map(|point| AccessPoint {
                    type_code: point.type_code(),
                    id: point.id(),
                }),
            },
            ActiveParticipant {
                user_id: self.destination.to_string(),
                alternative_user_id: Some(self.process_id.to_string()),
                user_name: None,
                user_is_requestor: false,
                role_id_codes: vec![CodedValue::of(DESTINATION_ROLE)],
                access_point: Some(host.clone()),
            },
        ];
        let mut objects: Vec<ParticipantObject> = self
            .patients
            .iter()
            .map(|patient| ParticipantObject {
                id: patient.clone(),
                type_code: Some("1"),
                type_code_role: Some("1"),
                data_life_cycle: None,
                // NOTE: §3.55.5.1.2 leaves the patient's ParticipantObjectIDTypeCode not
                // specialized and PS3.15 A.5.1 requires one: the RFC-3881 Patient Number.
                id_type_code: CodedValue::of(OBJECT_ID_PATIENT_NUMBER),
                query: None,
                details: Vec::new(),
            })
            .collect();
        objects.push(ParticipantObject {
            // NOTE: ITI TF-2 §3.55.5.1.2 leaves ParticipantObjectID optional and PS3.15
            // A.5.1 requires the attribute, so it is written empty.
            id: SecretString::from(String::new()),
            type_code: Some("2"),
            type_code_role: Some("24"),
            data_life_cycle: None,
            id_type_code: transaction.clone(),
            query: Some(self.query.clone()),
            details: self
                .home_community
                .iter()
                .map(|community| {
                    (
                        HOME_COMMUNITY_DETAIL.to_owned(),
                        SecretString::from(community.to_string()),
                    )
                })
                .collect(),
        });
        AuditMessage {
            event: EventIdentification {
                event_id: CodedValue::of(EVENT_ID),
                action: EVENT_ACTION,
                date_time: self.date_time,
                outcome: self.outcome.code(),
                type_codes: vec![transaction],
                outcome_description: None,
            },
            participants,
            source: source.clone(),
            objects,
        }
    }
}
