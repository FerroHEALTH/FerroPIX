// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The configuration tree, read by `ferrohealth-config`.
//!
//! The defaults, one TOML file, the environment overrides and the `--set`
//! arguments make one typed [`settings::Settings`] the run path holds.
//!
//! The file is the one `--config` or `FERROPIX_CONFIG` names, every key can
//! be overridden by `FERROPIX__<SECTION>__<KEY>` and by `--set key=value`,
//! and `RUST_LOG` sets `telemetry.filter`. Every table refuses an unknown
//! key, every default lives inline in its own `Default` impl, and every
//! secret is a `ferrohealth_secret` type read from its key or its
//! `<key>_file` sibling ([`secret`]). No specification governs the
//! configuration: our own design, in the family's shape.

pub mod error;
pub mod section;
pub mod settings;

use std::collections::BTreeMap;

use ferrohealth_config::loader::{Loaded, Loader};
use ferrohealth_secret::file::read_secret;
use ferrohealth_server::cli::ConfigArgs;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

use crate::config::error::Error;

/// The prefix of every environment override and of `FERROPIX_CONFIG`.
pub const PREFIX: &str = "FERROPIX";

/// Returns the loader of the server's configuration: the `FERROPIX`
/// namespace, with `RUST_LOG` as the alias of `telemetry.filter`.
///
/// # Errors
/// Returns [`Error::Load`] when the namespace is refused, which it never is
/// for [`PREFIX`].
pub fn loader() -> Result<Loader, Error> {
    Ok(Loader::new(PREFIX)?.alias("RUST_LOG", "telemetry.filter"))
}

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
    /// The console, the request log and the OTLP export (`[telemetry]`).
    pub telemetry: ferrohealth_telemetry::section::TelemetrySection,
    /// The PostgreSQL store (`[db]`).
    pub db: section::db::Db,
    /// The resource server (`[auth]`).
    pub auth: section::auth::Auth,
    /// The identifier domains seeded at start (`[[domain]]`).
    pub domain: Vec<section::domain::Domain>,
    /// The pseudonymisation domains seeded at start (`[[pseudonym_domain]]`).
    pub pseudonym_domain: Vec<section::pseudonym::PseudonymDomain>,
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
    /// The XCPD Responding Gateway (`[xcpd]`).
    pub xcpd: section::xcpd::Xcpd,
    /// The outbox's retention (`[outbox]`).
    pub outbox: section::outbox::Outbox,
    /// The BALP audit trail (`[audit]`).
    pub audit: section::audit::Audit,
    /// The operator API (`[operator]`).
    pub operator: section::operator::Operator,
    /// The installation report to `FerroPULSE` (`[usage_report]`).
    pub usage_report: section::usage_report::UsageReport,
    /// The development table, admitted under `profile = "development"` only
    /// (`[dev]`).
    pub dev: Option<section::dev::Dev>,
}

impl Config {
    /// Reads the configuration of the running process: the file `args`
    /// names (or `FERROPIX_CONFIG` does), the environment, and the `--set`
    /// pairs of `args`, last.
    ///
    /// # Errors
    /// Returns [`Error::Load`] with every refusal of the loader.
    pub fn load(args: &ConfigArgs) -> Result<Loaded<Self>, Error> {
        Ok(loader()?.load_with(args.config.as_deref(), &args.sets)?)
    }

    /// Reads `text` as the configuration file and applies `environment`
    /// over it, for a test that states both (setting a variable is `unsafe`
    /// in edition 2024).
    ///
    /// # Errors
    /// Returns [`Error::Load`] with every refusal of the loader.
    pub fn from_sources(
        text: Option<&str>,
        environment: &BTreeMap<String, String>,
    ) -> Result<Self, Error> {
        let loaded = loader()?.load_sources::<Self>(text, None, environment)?;
        Ok(loaded.into_parts().0)
    }
}

/// Reads the secret set under `key` or in the file its `<key>_file` sibling
/// names, as the redacting type `T`, or `None` when neither is set.
///
/// Under [`Profile::Production`] a value set under `key` itself is refused,
/// so a secret never sits in the configuration file or the environment of a
/// production server (no specification governs this: our own design).
///
/// # Errors
/// Returns [`Error::InlineSecret`] for an inline value under production and
/// [`Error::Secret`] with every refusal of `ferrohealth_secret::file`.
pub fn secret<T>(
    key: &str,
    inline: Option<T>,
    file: Option<&std::path::Path>,
    profile: Profile,
) -> Result<Option<T>, Error>
where
    T: From<SecretString>,
{
    if inline.is_some() && file.is_none() && profile == Profile::Production {
        return Err(Error::InlineSecret {
            key: key.to_owned(),
        });
    }
    Ok(read_secret(key, inline, file)?)
}
