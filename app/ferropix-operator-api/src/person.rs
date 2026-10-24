// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The person document, `GET {base}/operator/persons/{id}`: the person with
//! its identifiers per domain, masked, its source records side by side, its
//! history and its pseudonyms.
//!
//! An identifier and a pseudonym are shown masked; each carries the handle
//! a reveal names (`POST {base}/operator/persons/{id}/reveal`), so a value
//! the steward did not reveal never reaches the console.

use serde::{Deserialize, Serialize};

use crate::patient::{Channel, Demographics, DomainRef, Masked, PersonStatus};

/// The most entries [`Person::history`] holds, the newest first.
pub const HISTORY: usize = 200;

/// What `GET {base}/operator/persons/{id}` answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Person {
    /// The person's id, the FHIR `Patient.id` the PDQm Supplier serves.
    pub id: String,
    /// Whether the person is active or merged.
    pub status: PersonStatus,
    /// The person this one was merged into, when it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_into: Option<String>,
    /// When the person was made, RFC 3339 UTC.
    pub created_at: String,
    /// The demographics the PDQm Supplier serves for the person: an
    /// authoritative domain's record first, then the most recently received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading: Option<Demographics>,
    /// The identifiers, ordered by domain system.
    pub identifiers: Vec<IdentifierRow>,
    /// The source records, one per domain record, ordered by domain system.
    pub records: Vec<SourceRecord>,
    /// The history, the newest first, at most [`HISTORY`] entries.
    pub history: Vec<HistoryEntry>,
    /// Whether older history entries were left out.
    pub history_truncated: bool,
    /// Every pseudonymisation domain, with the person's pseudonym in it.
    pub pseudonyms: Vec<PseudonymRow>,
}

/// What a reveal names to unmask one identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum IdentifierHandle {
    /// The identifier of the source record with this id.
    Record(String),
    /// An authoritative identifier a record carries with no record of its
    /// own, by the id of the carried row.
    Carried(String),
}

/// The state of a source record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordState {
    /// The record is active and linked to the person.
    Active,
    /// The Source resolved the record as a duplicate of another (PIXm 3.1.0
    /// §2:3.104.4.2.3).
    Subsumed,
    /// The Source removed the record (PIXm 3.1.0 §2:3.104.4.3.3).
    Removed,
}

/// Why a record is linked to its person, as the store spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// The record's first person, made when it arrived alone.
    Feed,
    /// An authoritative identifier agreed.
    Deterministic,
    /// A probabilistic link.
    Probabilistic,
    /// A steward's decision.
    Steward,
    /// The Source's own statement.
    Source,
    /// Moved by a person merge.
    Merge,
}

/// The link of a record to the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// Why the record is linked.
    pub kind: LinkKind,
    /// Who made the link: a Source's client id or a steward.
    pub made_by: String,
    /// When the link was made, RFC 3339 UTC.
    pub made_at: String,
}

/// One identifier of the person, masked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentifierRow {
    /// What a reveal names to unmask this identifier.
    pub handle: IdentifierHandle,
    /// The identifier's domain.
    pub domain: DomainRef,
    /// The identifier, masked.
    pub masked: Masked,
    /// The state of the record the identifier belongs to.
    pub state: RecordState,
    /// When the record was last fed, RFC 3339 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fed: Option<String>,
    /// The record's link to the person; a carried identifier has none of
    /// its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Link>,
}

/// One source record of the person: what one domain says of the patient.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    /// The record's id.
    pub id: String,
    /// The record's domain.
    pub domain: DomainRef,
    /// The current version's number.
    pub version: u32,
    /// When the current version was received, RFC 3339 UTC.
    pub received_at: String,
    /// The channel the current version arrived on.
    pub channel: Channel,
    /// The record's state.
    pub state: RecordState,
    /// The demographics of the current version.
    pub demographics: Demographics,
}

/// One entry of the person's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    /// When it happened, RFC 3339 UTC.
    pub at: String,
    /// Who did it: a Source's client id or a steward, when one is recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// The reason given, when one was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// What happened.
    pub event: HistoryEvent,
}

/// What a history entry records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryEvent {
    /// A version of one of the person's records was received.
    Version {
        /// The record.
        record: String,
        /// The version's number.
        version: u32,
        /// The channel it arrived on.
        channel: Channel,
    },
    /// A record was linked to the person.
    Linked {
        /// The record.
        record: String,
        /// Why it was linked.
        kind: LinkKind,
    },
    /// A record's link to the person ended.
    Unlinked {
        /// The record.
        record: String,
    },
    /// A person was merged into another.
    Merged {
        /// The merge.
        merge: String,
        /// The person that survives.
        survivor: String,
        /// The person merged into it.
        merged: String,
    },
    /// A merge was reverted.
    Unmerged {
        /// The merge.
        merge: String,
        /// The person that survived it.
        survivor: String,
        /// The person restored.
        merged: String,
    },
    /// A steward action or a system action on the person, by its kind.
    Action {
        /// The action's kind: `view`, `reveal`, `link`, `merge` and the
        /// others the audit trail lists.
        kind: String,
    },
}

/// One pseudonymisation domain with the person's pseudonym in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PseudonymRow {
    /// The pseudonymisation domain's id, which a reveal names.
    pub domain: String,
    /// The domain's namespace.
    pub namespace: String,
    /// The domain's name.
    pub name: String,
    /// The person's pseudonym in the domain, masked, when one is minted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked: Option<Masked>,
}

#[cfg(test)]
mod tests {
    use super::{HistoryEvent, IdentifierHandle};
    use crate::patient::Channel;

    #[test]
    fn a_handle_and_an_event_are_tagged_by_their_snake_case_name() {
        let handle = IdentifierHandle::Carried(String::from("c-1"));
        assert_eq!(
            r#"{"carried":"c-1"}"#,
            serde_json::to_string(&handle).expect("it serializes")
        );
        let event = HistoryEvent::Version {
            record: String::from("r-1"),
            version: 2,
            channel: Channel::V2,
        };
        let text = serde_json::to_string(&event).expect("it serializes");
        assert_eq!(
            r#"{"version":{"record":"r-1","version":2,"channel":"v2"}}"#,
            text
        );
        assert_eq!(
            event,
            serde_json::from_str::<HistoryEvent>(&text).expect("it reads back")
        );
    }
}
