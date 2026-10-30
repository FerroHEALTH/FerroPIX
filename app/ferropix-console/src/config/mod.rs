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
//! authorization_endpoint = "https://idp.example.org/realms/ferropix/protocol/openid-connect/auth"
//! token_endpoint = "https://idp.example.org/realms/ferropix/protocol/openid-connect/token"
//! jwks_uri = "https://idp.example.org/realms/ferropix/protocol/openid-connect/certs"
//! client_id = "ferropix-console"
//! client_secret_file = "/run/secrets/console-client-secret"
//! redirect_uri = "https://console.example.org/auth/callback"
//! end_session_endpoint = "https://idp.example.org/realms/ferropix/protocol/openid-connect/logout"
//! post_logout_redirect_uri = "https://console.example.org/"
//! offline_access = true
//! ```

pub mod error;
pub mod settings;

use std::collections::BTreeMap;
use std::path::PathBuf;

use ferrohealth_config::loader::{Loaded, Loader};
use ferrohealth_secret::secret::Secret;
use ferrohealth_server::cli::ConfigArgs;
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
    /// `[server]`: where the console listens and where its site bundle is.
    pub server: Server,
    /// `[pix]`: the FerroPIX server the console is a client of.
    pub pix: Pix,
    /// `[session]`: the server-side sign-in sessions.
    pub session: Session,
    /// `[telemetry]`: the console log of `ferrohealth-telemetry`.
    pub telemetry: TelemetrySection,
    /// `[oidc]`: the OpenID Provider operators sign in with; without it the
    /// console offers no sign-in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc: Option<Oidc>,
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

/// `[session]`: the pending sign-ins and the signed-in sessions, each a pool
/// of its own.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Session {
    /// Whether the cookies carry `Secure`, so a browser sends them over HTTPS
    /// only; off only for a console on loopback.
    pub secure_cookie: bool,
    /// How long a sign-in begun at `GET /login` waits for the provider's
    /// redirect back, in seconds.
    pub sign_in_timeout_s: u64,
    /// How many pending sign-ins the console holds at once; a new one drops
    /// the oldest.
    pub max_sign_ins: usize,
    /// How long a signed-in session lives without a request, in seconds.
    pub idle_timeout_s: u64,
    /// How long a signed-in session lives at most, in seconds.
    pub absolute_timeout_s: u64,
    /// How many signed-in sessions the console holds at once; a sign-in past
    /// it is refused.
    pub max_sessions: usize,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            secure_cookie: true,
            sign_in_timeout_s: 300,
            max_sign_ins: 1000,
            idle_timeout_s: 1800,
            absolute_timeout_s: 43_200,
            max_sessions: 10_000,
        }
    }
}

/// `[oidc]`: an OAuth 2.0 authorization code client with PKCE at an OpenID
/// Provider.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Oidc {
    /// The provider's issuer identifier, which an ID Token's `iss` must equal
    /// exactly (OpenID Connect Core 1.0 §3.1.3.7).
    pub issuer: String,
    /// The provider's authorization endpoint (RFC 6749 §3.1).
    pub authorization_endpoint: String,
    /// The provider's token endpoint, where the authorization code is
    /// exchanged and the access token refreshed (RFC 6749 §3.2).
    pub token_endpoint: String,
    /// The provider's JWK Set, whose keys sign its ID Tokens (OpenID Connect
    /// Core 1.0 §10.1).
    pub jwks_uri: String,
    /// The console's client identifier at the provider (RFC 6749 §2.2).
    pub client_id: String,
    /// The console's client secret, inline, for a confidential client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<Secret>,
    /// A file holding the console's client secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret_file: Option<PathBuf>,
    /// The redirection endpoint the provider sends the operator back to
    /// (RFC 6749 §3.1.2), the console's `/auth/callback`.
    pub redirect_uri: String,
    /// The scopes requested, `openid` among them.
    pub scopes: Vec<String>,
    /// Whether the sign-in asks for `offline_access` (OpenID Connect Core
    /// 1.0 §11), so the provider issues a refresh token the console renews
    /// the operator's access token with.
    pub offline_access: bool,
    /// The provider's end-session endpoint, where a sign-out sends the
    /// operator (OpenID Connect RP-Initiated Logout 1.0 §2); without it a
    /// sign-out ends the console's session alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_session_endpoint: Option<String>,
    /// Where the provider sends the operator after it signed them out, as
    /// registered with the provider (OpenID Connect RP-Initiated Logout 1.0
    /// §3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_logout_redirect_uri: Option<String>,
}

impl Default for Oidc {
    fn default() -> Self {
        Self {
            issuer: String::new(),
            authorization_endpoint: String::new(),
            token_endpoint: String::new(),
            jwks_uri: String::new(),
            client_id: String::new(),
            client_secret: None,
            client_secret_file: None,
            redirect_uri: String::new(),
            scopes: vec![String::from("openid")],
            offline_access: false,
            end_session_endpoint: None,
            post_logout_redirect_uri: None,
        }
    }
}
