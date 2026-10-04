// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! An ITI-93 Mobile Patient Identity Feed message as a Patient Identity
//! Consumer reads it (§2:3.93.4.1), and the response it returns
//! (§2:3.93.4.2).
//!
//! [`Feed::read`] holds the message to the PMIR Bundle, `MessageHeader`,
//! history Bundle and merged Patient profiles and keeps, of every `Patient`
//! it carries, only what identifies the Patient Master Identity: its
//! resource id and its business identifiers, each value in a
//! [`SecretString`]. Names, addresses, contact points and every other
//! demographic are dropped as the message is read. None of the types here
//! has `Display`, and `Debug` shows no value.

use std::fmt;

use secrecy::SecretString;

use crate::redact::REDACTED;

use super::error::{FeedError, InvalidInput};
use super::message;

/// The kind of change one history entry reports (§2:3.93.4.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum EventKind {
    /// A Patient Master Identity was created (`POST`).
    Create,
    /// A Patient Master Identity was updated (`PUT`, no `replaced-by` link).
    Update,
    /// A Patient Master Identity was deleted (`DELETE`).
    Delete,
    /// A Patient Master Identity was merged into another one (`PUT` of the
    /// deprecated Patient with a `replaced-by` link, §2:3.93.4.1.2.4).
    Merge,
}

impl EventKind {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 4] = [Self::Create, Self::Update, Self::Delete, Self::Merge];

    /// The kind's name, for a log field or a metric label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Merge => "merge",
        }
    }
}

/// One business identifier of a Patient Master Identity (`Patient.identifier`).
///
/// `Debug` shows the system and never the value.
#[derive(Clone)]
pub struct Identifier {
    system: Option<String>,
    value: SecretString,
}

impl Identifier {
    /// Pairs `system`, the assigning authority, with `value`.
    #[must_use]
    pub fn new(system: Option<String>, value: SecretString) -> Self {
        Self { system, value }
    }

    /// The assigning authority (`Identifier.system`), when the identifier
    /// names one.
    #[must_use]
    pub fn system(&self) -> Option<&str> {
        self.system.as_deref()
    }

    /// The identifier value, which never leaves this type unredacted unless
    /// the caller exposes it.
    #[must_use]
    pub fn value(&self) -> &SecretString {
        &self.value
    }
}

impl fmt::Debug for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identifier")
            .field("system", &self.system)
            .field("value", &REDACTED)
            .finish()
    }
}

/// What identifies one Patient Master Identity: the Registry's resource id
/// and the business identifiers (§2:3.93.4.1.1).
///
/// `Debug` shows how many identifiers there are, their systems, and no
/// value.
#[derive(Clone, Default)]
pub struct PatientIdentity {
    id: Option<SecretString>,
    identifiers: Vec<Identifier>,
}

impl PatientIdentity {
    /// The identity with resource id `id` and `identifiers`.
    #[must_use]
    pub fn new(id: Option<SecretString>, identifiers: Vec<Identifier>) -> Self {
        Self { id, identifiers }
    }

    /// The resource id the Registry assigned (`Patient.id`), when the entry
    /// carries one.
    #[must_use]
    pub fn id(&self) -> Option<&SecretString> {
        self.id.as_ref()
    }

    /// The business identifiers (`Patient.identifier`), in message order;
    /// empty for a delete that carries no resource.
    #[must_use]
    pub fn identifiers(&self) -> &[Identifier] {
        &self.identifiers
    }
}

impl fmt::Debug for PatientIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let systems: Vec<Option<&str>> = self.identifiers.iter().map(Identifier::system).collect();
        f.debug_struct("PatientIdentity")
            .field("id", &self.id.as_ref().map(|_| REDACTED))
            .field("identifier_systems", &systems)
            .finish()
    }
}

/// One change a history entry reports.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    /// A Patient Master Identity was created.
    Created(PatientIdentity),
    /// A Patient Master Identity was updated: the identity as it now is,
    /// which never says what it lost.
    Updated(PatientIdentity),
    /// A Patient Master Identity was deleted: its id from the request, and
    /// its identifiers when the entry carries the resource.
    Deleted(PatientIdentity),
    /// A Patient Master Identity was merged into the surviving one.
    Merged {
        /// The deprecated Patient (`active` false), with its identifiers.
        subsumed: PatientIdentity,
        /// The reference to the surviving Patient (`link.other`).
        surviving: SecretString,
    },
}

impl Event {
    /// The kind of change.
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        match self {
            Self::Created(_) => EventKind::Create,
            Self::Updated(_) => EventKind::Update,
            Self::Deleted(_) => EventKind::Delete,
            Self::Merged { .. } => EventKind::Merge,
        }
    }
}

/// An ITI-93 message, read and held to the PMIR profiles.
#[derive(Debug, Clone)]
pub struct Feed {
    message_id: String,
    events: Vec<Event>,
}

impl Feed {
    /// Reads the message `body` that arrived with media type `media`.
    ///
    /// The message is a FHIR JSON `Bundle` of type `message` with exactly two
    /// entries: a `MessageHeader` whose event is
    /// `urn:ihe:iti:pmir:2019:patient-feed`, with a destination and one focus
    /// on the second entry, and a `history` Bundle of unique, successful
    /// `Patient` creates, updates, merges and deletes (§2:3.93.4.1.2). A
    /// Patient with a `replaced-by` link is a merge and holds to the merged
    /// Patient profile: `active` false, that one link, sent as an update
    /// (§2:3.93.4.1.2.4).
    ///
    /// # Errors
    /// A [`FeedError`] for a message that departs from any of those rules;
    /// none of it is read further.
    pub fn read(media: Option<&str>, body: &[u8]) -> Result<Self, FeedError> {
        let (message_id, events) = message::read(media, body)?;
        Ok(Self { message_id, events })
    }

    /// The `MessageHeader.id` of the message, which the response names.
    #[must_use]
    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    /// The changes, in history order.
    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// How many changes of `kind` the message reports.
    #[must_use]
    pub fn count(&self, kind: EventKind) -> usize {
        self.events
            .iter()
            .filter(|event| event.kind() == kind)
            .count()
    }

    /// The Mobile Patient Identity Feed Response to this message: a `Bundle`
    /// of type `message` with one `MessageHeader` that reports `ok`
    /// (§2:3.93.4.2.2, the PMIR `MessageHeader` response profile), as FHIR
    /// JSON.
    ///
    /// `response_id` is the new message's id, which also names its entry as a
    /// `urn:uuid:`; `source` is the endpoint of the Consumer that answers.
    ///
    /// # Errors
    /// [`Unwritable`] when the response cannot be written as FHIR JSON.
    pub fn acknowledgement(
        &self,
        response_id: &ResponseId,
        source: &str,
    ) -> Result<Vec<u8>, Unwritable> {
        message::acknowledgement(&self.message_id, response_id.as_str(), source)
    }
}

/// The id of a response message: a UUID in its lowercase hyphenated form
/// (RFC 9562 §4), which also names the response's entry as a `urn:uuid:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseId(String);

impl ResponseId {
    /// Reads `text` as a response id.
    ///
    /// # Errors
    /// [`InvalidInput::ResponseId`] when `text` is not a lowercase hyphenated
    /// UUID.
    pub fn new(text: &str) -> Result<Self, InvalidInput> {
        let groups: Vec<&str> = text.split('-').collect();
        let shaped = groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
            && text
                .chars()
                .all(|character| character == '-' || matches!(character, '0'..='9' | 'a'..='f'));
        if shaped {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidInput::ResponseId)
        }
    }

    /// The id as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A response or a refusal that cannot be written as FHIR JSON.
#[derive(Debug, thiserror::Error)]
#[error("the response message cannot be written as FHIR JSON")]
pub struct Unwritable(#[source] pub(super) serde_json::Error);

/// Returns the body that refuses a message for `error`.
///
/// It is an `OperationOutcome` with one `error` issue of
/// [`FeedError::issue`], whose text is the error's own and names no value
/// (FHIR R4 http.html, §2:3.93.4.2.2).
///
/// # Errors
/// [`Unwritable`] when the outcome cannot be written as FHIR JSON.
pub fn refusal(error: &FeedError) -> Result<Vec<u8>, Unwritable> {
    message::refusal(error)
}
