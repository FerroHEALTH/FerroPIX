// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A steward's merge of two persons and its revert:
//! `GET {base}/operator/merges/preview`, `POST {base}/operator/merges` and
//! `POST {base}/operator/merges/{id}/revert`.
//!
//! The preview answers the whole effect and writes nothing; the merge and
//! the revert carry the versions the steward read, so a decision made on a
//! stale screen is refused with `409`. Only the latest merge whose survivor
//! is a person can be reverted, so reverts unwind in order. No specification
//! governs the merge screens: our own design; PMIR 1.6.0 §2:3.93.4.1.3
//! admits the local unmerge policy.

use serde::{Deserialize, Serialize};

use crate::candidate::PersonIdentifier;
use crate::patient::Masked;

/// The query of `GET {base}/operator/merges/preview`: the two persons of a
/// merge, or the merge a revert would undo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewQuery {
    /// The person that would survive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub survivor: Option<String>,
    /// The person that would be merged into it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged: Option<String>,
    /// The merge a revert would undo, in place of the two persons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revert: Option<String>,
}

/// One person of a merge, as the preview names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Party {
    /// The person's id.
    pub id: String,
    /// The person's version, which a merge or a revert carries back.
    pub version: String,
    /// The leading name, when the person's records state one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// When the person was created, RFC 3339 UTC.
    pub created_at: String,
}

/// A pseudonym that resolves to another person after the change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PseudonymMove {
    /// The pseudonymisation domain's namespace.
    pub namespace: String,
    /// The domain's name.
    pub name: String,
    /// The pseudonym, masked.
    pub masked: Masked,
}

/// What `GET {base}/operator/merges/preview` answers with: the whole effect
/// of a merge, or of a revert, written nowhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    /// The person that survives the merge.
    pub survivor: Party,
    /// The person merged into it, or reactivated by a revert.
    pub merged: Party,
    /// The merge a revert undoes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revert: Option<String>,
    /// The reason the merge was made, for a revert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// For a revert, the later merge of the survivor that has to be
    /// reverted first, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_revert: Option<String>,
    /// The identifiers that move from one person to the other, masked.
    pub identifiers: Vec<PersonIdentifier>,
    /// The pseudonyms that resolve to the other person afterwards.
    pub pseudonyms: Vec<PseudonymMove>,
    /// The PMIR subscriptions the change is announced to.
    pub subscriptions: u64,
    /// The candidates between the two persons' records the merge closes as
    /// linked.
    pub candidates: Vec<String>,
}

/// The body of `POST {base}/operator/merges`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewMerge {
    /// The person that survives.
    pub survivor: String,
    /// The survivor's version as the steward read it.
    pub survivor_version: String,
    /// The person merged into it.
    pub merged: String,
    /// The merged person's version as the steward read it.
    pub merged_version: String,
    /// Why, required and at most 500 characters.
    pub reason: String,
}

/// The body of `POST {base}/operator/merges/{id}/revert`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revert {
    /// The survivor's version as the steward read it.
    pub survivor_version: String,
    /// Why, required and at most 500 characters.
    pub reason: String,
}

/// What a merge or a revert answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    /// The merge made, or reverted.
    pub merge: String,
    /// The steward action that records it.
    pub action: String,
    /// The surviving person.
    pub survivor: String,
    /// The person merged, or reactivated.
    pub merged: String,
    /// How many identifiers moved.
    pub moved: u64,
}
