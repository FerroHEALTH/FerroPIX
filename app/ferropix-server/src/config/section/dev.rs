// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[dev]` table: the development profile's own settings, admitted under
//! `profile = "development"` only.
//!
//! The development profile is for a run on a developer's machine, with Basic
//! users and no TLS; `loopback` keeps such a server off every network
//! interface but the loopback one. No specification governs this: our own
//! design.

use serde::{Deserialize, Serialize};

/// The `[dev]` table as written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Dev {
    /// Whether every listener must bind a loopback address.
    pub loopback: bool,
}

impl Default for Dev {
    fn default() -> Self {
        Self { loopback: true }
    }
}
