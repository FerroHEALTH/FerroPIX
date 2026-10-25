// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The configuration tree: one TOML file, then the environment overrides,
//! then one typed [`settings::Settings`] the run path holds.
//!
//! The file is the one `--config` or `FERROPIX_CONFIG` names, and every key
//! can be overridden by `FERROPIX__<SECTION>__<KEY>`. Every table refuses an
//! unknown key, every default lives inline in its own `Default` impl, and
//! every secret is a [`secret::Secret`] or [`secret::SecretUrl`] read from its
//! key or its `<key>_file` sibling. No specification governs the
//! configuration: our own design, in the family's shape.

pub mod error;
mod load;
pub mod origin;
pub mod secret;
pub mod section;
pub mod settings;

use serde::{Deserialize, Serialize};

/// The prefix of every environment override.
///
/// The name after it is the dotted key with `__` between levels, the key in
/// either case: `FERROPIX__SERVER__LISTEN` sets `[server] listen`.
pub const ENV_PREFIX: &str = "FERROPIX__";

/// The environment variable naming the configuration file.
pub const CONFIG_PATH_ENV: &str = "FERROPIX_CONFIG";

/// The deployment profile.
///
/// `production` is the default and refuses every setting made for a run on a
/// developer's machine: the `[dev]` table, Basic users, an inline secret, a
/// plain `http` public base URL and an unencrypted audit repository.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// A deployment that holds patient data.
    #[default]
    Production,
    /// A run on a developer's machine, with the `[dev]` table.
    Development,
}

/// The whole configuration tree, as the file and the environment state it.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The deployment profile.
    pub profile: Profile,
    /// The HTTP surface (`[server]`).
    pub server: section::server::Server,
    /// The `tracing` console and the OTLP endpoint (`[telemetry]`).
    pub telemetry: section::telemetry::Telemetry,
    /// The PostgreSQL store (`[db]`).
    pub db: section::db::Db,
    /// The resource server (`[auth]`).
    pub auth: section::auth::Auth,
    /// The identifier domains seeded at start (`[[domain]]`).
    pub domain: Vec<section::domain::Domain>,
    /// The scorer's thresholds, weights and blocking keys (`[matching]`).
    pub matching: section::matching::Matching,
    /// The PIXm Manager (`[pixm]`).
    pub pixm: section::profiles::Pixm,
    /// The PDQm Supplier (`[pdqm]`).
    pub pdqm: section::profiles::Pdqm,
    /// The PMIR Registry (`[pmir]`).
    pub pmir: section::profiles::Pmir,
    /// The HL7 v2 face (`[hl7v2]`).
    pub hl7v2: section::hl7v2::Hl7v2,
    /// The BALP audit trail (`[audit]`).
    pub audit: section::audit::Audit,
    /// The operator API (`[operator]`).
    pub operator: section::operator::Operator,
    /// The installation report to `FerroPULSE` (`[usage_report]`).
    pub usage_report: section::usage_report::UsageReport,
    /// The development table, admitted under `profile = "development"` only
    /// (`[dev]`).
    pub dev: Option<section::dev::Dev>,
    /// Where each setting came from, recorded as the tree is read.
    #[serde(skip)]
    pub origins: origin::Origins,
}
