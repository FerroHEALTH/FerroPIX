// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's configuration, read by `ferrohealth-config`.
//!
//! The layers are the defaults, a TOML file, the `FERROPIX_CONSOLE__`
//! environment overrides and the `--set` arguments, and every secret is
//! reachable through a `<key>_file` sibling read at start.
//!
//! [`Config`] is the tree as written, every default in its struct's
//! `Default` impl; [`Config::load`] reads it and
//! [`Config::resolve`](crate::config::Config::resolve) turns it into the
//! checked [`settings::Settings`] the server runs on. An override names its
//! key by path, `FERROPIX_CONSOLE__PIX__BASE_URL` for `[pix]
//! base_url`, and `RUST_LOG` sets `telemetry.filter`. No specification
//! governs the configuration: our own design.
//!
//! ```toml
//! [server]
//! listen = "0.0.0.0:3000"
//! locale = "en"
//!
//! [pix]
//! base_url = "https://pix.example.org/"
//! timeout_ms = 30000
//!
//! [oidc]
//! issuer = "https://idp.example.org/realms/ferropix"
//! client_id = "ferropix-console"
//! client_secret_file = "/run/secrets/console-client-secret"
//! redirect_uri = "https://console.example.org/auth/callback"
//! post_logout_redirect_uri = "https://console.example.org/"
//! offline_access = true
//! ```
//!
//! `[oidc]` and `[session]` are the sections of `ferrohealth-session`: with
//! `issuer` alone the provider's endpoints are discovered when the console
//! starts, and each endpoint written takes the place of the discovered one.

pub mod error;
pub mod settings;

use std::collections::BTreeMap;

use ferrohealth_config::loader::{Loaded, Loader};
use ferrohealth_server::cli::ConfigArgs;
use ferrohealth_session::config::{OidcSection, SessionSection};
use ferrohealth_telemetry::section::TelemetrySection;
use serde::{Deserialize, Serialize};

use crate::config::error::Error;

/// The prefix of every environment override and of
/// `FERROPIX_CONSOLE_CONFIG`.
pub const PREFIX: &str = "FERROPIX_CONSOLE";

/// Returns the loader of the console's configuration: the
/// `FERROPIX_CONSOLE` namespace, with `RUST_LOG` as the alias of
/// `telemetry.filter`.
///
/// # Errors
/// Returns [`Error::Load`] when the namespace is refused, which it never is
/// for [`PREFIX`].
pub fn loader() -> Result<Loader, Error> {
    Ok(Loader::new(PREFIX)?.alias("RUST_LOG", "telemetry.filter"))
}

impl Config {
    /// Reads the configuration of the running process: the file `args`
    /// names (or `FERROPIX_CONSOLE_CONFIG` does), the environment, and the
    /// `--set` pairs of `args`, last.
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

/// The configuration tree as written.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// `[server]`: where the console listens and the language a page falls
    /// back to.
    pub server: Server,
    /// `[pix]`: the FerroPIX server the console is a client of.
    pub pix: Pix,
    /// `[session]`: the store and the lifetimes of the sign-in sessions.
    pub session: SessionSection,
    /// `[telemetry]`: the console log of `ferrohealth-telemetry`.
    pub telemetry: TelemetrySection,
    /// `[oidc]`: the OpenID Provider operators sign in with; without it the
    /// console offers no sign-in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc: Option<OidcSection>,
}

/// `[server]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Server {
    /// The socket address the console listens on.
    pub listen: String,
    /// The BCP 47 tag of the locale a page renders in when the request's
    /// `Accept-Language` names no language the console's catalog holds.
    pub locale: String,
}

impl Default for Server {
    fn default() -> Self {
        Self {
            listen: String::from("127.0.0.1:3000"),
            locale: String::from("en"),
        }
    }
}

/// `[pix]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pix {
    /// The root of the FerroPIX server's HTTP surface, under which it serves
    /// `/health`, `/fhir` and `/operator`.
    pub base_url: String,
    /// How long one call to the server may take, in milliseconds.
    pub timeout_ms: u64,
    /// The scope the server's `[operator] admin_scope` names: the console draws
    /// an administrator's forms only when the provider granted it.
    pub admin_scope: String,
}

impl Default for Pix {
    fn default() -> Self {
        Self {
            base_url: String::from("http://127.0.0.1:8080/"),
            timeout_ms: 30_000,
            admin_scope: String::from("__ferropix.admin"),
        }
    }
}
