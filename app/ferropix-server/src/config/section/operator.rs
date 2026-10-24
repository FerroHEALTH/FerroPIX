// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[operator]` section: the switch of the steward's API.
//!
//! The API is mounted under `{base}/operator`, off by default. Every request
//! to it carries a token of an `[auth]` issuer with the steward or the
//! administrator scope, so it has no credential of its own. No specification governs this: our own design.

use serde::{Deserialize, Serialize};

/// The `[operator]` section as written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Operator {
    /// Whether the operator API is mounted.
    pub enabled: bool,
}
