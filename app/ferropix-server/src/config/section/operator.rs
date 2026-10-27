// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[operator]` section: the switch of the steward's API and the scope
//! of an administrator.
//!
//! The API is mounted under `{base}/operator`, off by default. Every request
//! to it carries a token of an `[auth]` issuer with the steward scope
//! (`[auth] product_scope`) or the administrator scope, so it has no
//! credential of its own. No specification governs this: our own design.

use serde::{Deserialize, Serialize};

/// The `[operator]` section as written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Operator {
    /// Whether the operator API is mounted.
    pub enabled: bool,
    /// The scope of an administrator, in an extension form of SMART App
    /// Launch 2.2.0 and other than the steward scope.
    pub admin_scope: String,
}

impl Default for Operator {
    fn default() -> Self {
        Self {
            enabled: false,
            admin_scope: String::from("__ferropix.admin"),
        }
    }
}
