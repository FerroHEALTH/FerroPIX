// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The audit trail: `GET {base}/operator/actions`, one page of steward and
//! system actions filtered by actor, kind, person and time, and
//! `POST {base}/operator/actions/export`, the same filter answered as CSV.

use serde::{Deserialize, Serialize};

/// The actions a page holds when the request names no limit.
pub const DEFAULT_LIMIT: u32 = 50;

/// The most actions a page holds, whatever the request asks.
pub const MAX_LIMIT: u32 = 200;

/// The most actions one export answers.
pub const EXPORT_LIMIT: u32 = 10_000;

/// The header an export names the `after` of the rest with, when it was cut
/// at [`EXPORT_LIMIT`].
pub const EXPORT_NEXT: &str = "x-ferropix-next";

/// One action of the trail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    /// The action's id.
    pub id: String,
    /// The action's kind: `view`, `reveal`, `export`, `domain-create`,
    /// `domain-update`, `feed`, `link`, `merge`, `candidate`, `remove` and
    /// `resolve-duplicate`.
    pub kind: String,
    /// Who acted: a steward or a Source's client id.
    pub actor: String,
    /// When, RFC 3339 UTC.
    pub at: String,
    /// The person the action concerns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
    /// The record the action concerns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<String>,
    /// The candidate the action recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate: Option<String>,
    /// The merge the action made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge: Option<String>,
    /// The identifier domain the action wrote or whose identifier it
    /// revealed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// The pseudonymisation domain whose pseudonym the action revealed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pseudonym_domain: Option<String>,
    /// The reason given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The request the action was made in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// What `GET {base}/operator/actions` answers with: one page, the newest
/// first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actions {
    /// The actions of this page.
    pub actions: Vec<Action>,
    /// The `after` of the next page, when actions follow this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

/// The filter of the trail: the query of `GET {base}/operator/actions` and
/// the body of `POST {base}/operator/actions/export`.
///
/// Every field narrows the trail; none names a patient identifier, and the
/// person is the id FerroPIX minted.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActionFilter {
    /// Only this actor's actions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// Only actions of this kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Only actions on this person.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
    /// Only actions at or after this instant, RFC 3339.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Only actions before this instant, RFC 3339.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// The id of the last action of the page before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// The actions a page holds, at most [`MAX_LIMIT`], or
    /// [`EXPORT_LIMIT`] for an export.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}
