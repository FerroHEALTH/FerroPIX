// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The sources: the systems allowed to feed records.
//!
//! `GET {base}/operator/sources` lists them and
//! `GET {base}/operator/sources/{id}` is one with its last messages. A
//! source is a FHIR client an issuer vouches for, a Basic user of the
//! development profile, or an HL7 v2 sender; how each is admitted is
//! configuration, read from the server's file. A message row names its
//! time, its channel, what it did and its outcome, never a value.

use serde::{Deserialize, Serialize};

use crate::patient::Channel;

/// The most messages [`SourcePage::messages`] lists.
pub const MESSAGES: usize = 20;

/// How a source is admitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Admission {
    /// A client the issuer `issuer` lists in its `clients`.
    IdentityProvider {
        /// The issuer.
        issuer: String,
    },
    /// A Basic user of the development profile.
    BasicUser,
    /// An HL7 v2 sender `[[hl7v2.senders]]` names.
    RegisteredSender {
        /// The sending application, MSH-3.
        application: String,
        /// The sending facility, MSH-4.
        facility: String,
    },
}

/// One source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// The source's id in the operator API: a client id, or an HL7 v2
    /// sender's facility and application joined by `.`.
    pub id: String,
    /// The name the source is shown by: a client id, or an HL7 v2 sender's
    /// facility and application joined by `|`.
    pub name: String,
    /// The channel it feeds on.
    pub channel: Channel,
    /// How it is admitted.
    pub admission: Admission,
    /// The systems of the identifier domains that name it as their Source.
    pub domains: Vec<String>,
    /// When it last fed a message, RFC 3339 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity: Option<String>,
}

/// What `GET {base}/operator/sources` answers with, ordered by id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sources {
    /// The sources.
    pub sources: Vec<Source>,
    /// The configuration file the sources are set in, by its name, when the
    /// server read one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// One message a source sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    /// When it was received, RFC 3339 UTC.
    pub at: String,
    /// The channel it came on.
    pub channel: Channel,
    /// What it asked: `feed`, `resolve-duplicate` or `remove` for a FHIR
    /// feed, `message` for an HL7 v2 message.
    pub action: String,
    /// How it ended: `applied`, or the HL7 v2 acknowledgment code.
    pub outcome: String,
}

/// What `GET {base}/operator/sources/{id}` answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePage {
    /// The source.
    pub source: Source,
    /// Its last messages, newest first, at most [`MESSAGES`].
    pub messages: Vec<Message>,
}
