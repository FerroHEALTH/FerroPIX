// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The matching configuration, `GET {base}/operator/configuration`.
//!
//! FerroPIX keeps no configuration store, so the configuration is read from
//! its file at start and shown read-only: each setting with its value and
//! where the value came from, the file named once (FerroUI 0.3.7
//! `design/COMPONENTS.md` rule 13, file mode).

use serde::{Deserialize, Serialize};

/// How the configuration is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Set in the file, read at start; a change takes effect at the next
    /// start.
    File,
}

/// Where a setting's value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The configuration file.
    File,
    /// An environment variable, which wins over the file.
    Environment,
    /// Nothing sets it: the built-in default.
    Default,
    /// Built into FerroPIX, not configurable.
    BuiltIn,
}

/// One setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    /// Its key in the file, a dotted path, or the rule's name for a
    /// built-in rule.
    pub key: String,
    /// Its value, written as TOML writes it.
    pub value: String,
    /// Where the value came from.
    pub origin: Origin,
}

/// One tab of settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    /// The section: `thresholds`, `weights`, `blocking` or `rules`.
    pub name: String,
    /// Its settings, in order.
    pub settings: Vec<Setting>,
}

/// What `GET {base}/operator/configuration` answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    /// How the configuration is kept.
    pub mode: Mode,
    /// The configuration file, by its name, when the server read one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// The sections, in order.
    pub sections: Vec<Section>,
}
