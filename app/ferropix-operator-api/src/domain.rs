// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The identifier domains.
//!
//! `GET {base}/operator/domains` is the listing and
//! `GET {base}/operator/domains/{id}` one domain with its last feeds. A
//! feed row names its time, its channel and what it did, never a value.

use serde::{Deserialize, Serialize};

use crate::patient::{Channel, DomainKind};

/// The domains a page holds when the request names no limit.
pub const DEFAULT_LIMIT: u32 = 50;

/// The most domains a page holds, whatever the request asks.
pub const MAX_LIMIT: u32 = 200;

/// The most feeds [`DomainPage::feeds`] lists.
pub const FEEDS: usize = 20;

/// One identifier domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Domain {
    /// The domain's id.
    pub id: String,
    /// The domain's system, the FHIR `Identifier.system`.
    pub system: String,
    /// The domain's name.
    pub name: String,
    /// What the domain is.
    pub kind: DomainKind,
    /// Whether its identifier values are sealed at rest.
    pub sensitive: bool,
    /// The client id of its one Patient Identity Source.
    pub source_client_id: String,
    /// When the domain was made, RFC 3339 UTC.
    pub created_at: String,
    /// When the domain was retired, RFC 3339 UTC; a retired domain answers
    /// as unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at: Option<String>,
    /// The source records the domain holds.
    pub records: u64,
    /// When a record of the domain was last fed, RFC 3339 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fed: Option<String>,
}

/// What `GET {base}/operator/domains` answers with: one page of domains,
/// ordered by system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Domains {
    /// The domains of this page.
    pub domains: Vec<Domain>,
    /// The `after` of the next page, when domains follow this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

/// The query of `GET {base}/operator/domains`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DomainQuery {
    /// The system of the last domain of the page before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// The domains a page holds, at most [`MAX_LIMIT`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// What a feed did to a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedOutcome {
    /// The record was created.
    Created,
    /// The record was revised.
    Revised,
}

/// One feed the domain received.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feed {
    /// When it was received, RFC 3339 UTC.
    pub at: String,
    /// The channel it arrived on.
    pub channel: Channel,
    /// What it did.
    pub outcome: FeedOutcome,
}

/// What `GET {base}/operator/domains/{id}` answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainPage {
    /// The domain.
    pub domain: Domain,
    /// Its last feeds, the newest first, at most [`FEEDS`].
    pub feeds: Vec<Feed>,
}

/// The body of `POST {base}/operator/domains`, behind the administrator
/// scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewDomain {
    /// The domain's system, an absolute URI.
    pub system: String,
    /// The domain's name.
    pub name: String,
    /// What the domain is: `local` or `authoritative`.
    pub kind: DomainKind,
    /// The client id of its one Patient Identity Source.
    pub source_client_id: String,
    /// Whether its identifier values are sealed at rest; `true` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitive: Option<bool>,
    /// Why the domain is added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The body of `PATCH {base}/operator/domains/{id}`, behind the
/// administrator scope: the fields it names change, the others stay.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DomainChange {
    /// The new name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The client id of the new Patient Identity Source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_client_id: Option<String>,
    /// `true` retires the domain, which then answers as unknown; a retired
    /// domain stays retired.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub retire: bool,
    /// Why the domain changes: required.
    pub reason: String,
}
