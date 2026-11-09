// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[terminology]` section: the FHIR R4 terminology server the HL7 v2
//! face translates every v2 table value through.
//!
//! A PID maps to a `Patient`, and a `Patient` back to a PID, through the
//! HL7 v2-to-FHIR 1.0.0 structure maps `ferrobridge-hl7v2` is built with;
//! each table value (PID-8 by table 0001, XPN-7 by table 0200, XAD-7 by
//! table 0190, XTN-2 and XTN-3 by tables 0201 and 0202) is translated by
//! `ConceptMap/$translate` (FHIR R4 `conceptmap-operation-translate`) on
//! this server, loaded with the guide's table maps and FerroBRIDGE's value
//! package. FerroPIX holds no copy of a table. The section has the
//! family's shape, as FerroBRIDGE's `[terminology]`: the base URL, the
//! timeout and retry budget, `[terminology.credentials]` of
//! `ferrohealth-smart` (HTTP Basic or the client credentials grant) and
//! `[terminology.tls]` of `ferrohealth-server`. No specification governs
//! the section: our own design.

use std::sync::Arc;
use std::time::Duration;

use ferrobridge_term::client::Client;
use ferrobridge_term::config::{Config, Credentials, RetryPolicy, WireVersion};
use ferrobridge_term::smart::SmartToken;
use ferrohealth_config::tls::TlsClientSection;
use ferrohealth_server::tls_client::TlsClient;
use ferrohealth_smart::client::cache::TokenSource;
use ferrohealth_smart::config::credentials::{CredentialsSection, Outbound};
use serde::{Deserialize, Serialize};

use crate::config::Profile;
use crate::config::error::Error;
use crate::config::section::{is_loopback_url, url};

/// The `[terminology]` section as written.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Terminology {
    /// The FHIR R4 service base URL; empty for no terminology server.
    pub base_url: String,
    /// How long one call may take, connection included.
    pub timeout_ms: u64,
    /// The retry budget.
    pub retry: Retry,
    /// The credentials the server expects.
    pub credentials: CredentialsSection,
    /// The outbound TLS: the client identity presented for mutual TLS and
    /// the roots trusted beside the platform's.
    pub tls: TlsClientSection,
}

impl Default for Terminology {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            timeout_ms: 10_000,
            retry: Retry::default(),
            credentials: CredentialsSection::default(),
            tls: TlsClientSection::default(),
        }
    }
}

/// How often, and how far apart, a call is sent again.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Retry {
    /// How many times a call is sent at most, the first try included.
    pub max_attempts: u32,
    /// The wait before the second attempt.
    pub initial_backoff_ms: u64,
    /// The longest wait between two attempts.
    pub max_backoff_ms: u64,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff_ms: 200,
            max_backoff_ms: 2_000,
        }
    }
}

/// The `[terminology]` section, resolved: the client of the server.
#[derive(Debug, Clone)]
pub struct TerminologySettings {
    /// The client every translation goes through.
    pub client: Client,
}

impl Terminology {
    /// Resolves the section: `None` while `base_url` is empty.
    ///
    /// # Errors
    /// Returns [`Error::Url`] for a base URL that does not parse,
    /// [`Error::Value`] for a base URL that is neither `https` nor `http` on
    /// a loopback host or that carries userinfo, and for a zero bound,
    /// [`Error::InlineSecret`] for a password or a client secret set inline
    /// under production, [`Error::Credentials`] for a credentials table
    /// `ferrohealth-smart` refuses, [`Error::TlsClient`] when
    /// `[terminology.tls]` names material that cannot be read,
    /// [`Error::Token`] when the token source cannot be built, and
    /// [`Error::HttpClient`] or [`Error::Terminology`] when the client
    /// cannot be built.
    pub fn resolve(&self, profile: Profile) -> Result<Option<TerminologySettings>, Error> {
        let text = self.base_url.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let base = url("terminology.base_url", text)?;
        // NOTE: FHIR R4 security.html leaves transport security to the deployment;
        // our own design: plain http only on loopback, as for every outbound URL.
        if !(base.scheme() == "https" || (base.scheme() == "http" && is_loopback_url(&base))) {
            return Err(Error::Value {
                key: String::from("terminology.base_url"),
                rule: "https, or http on a loopback host",
            });
        }
        if !base.username().is_empty() || base.password().is_some() {
            return Err(Error::Value {
                key: String::from("terminology.base_url"),
                rule: "no userinfo: credentials go in [terminology.credentials]",
            });
        }
        if self.timeout_ms == 0 || self.retry.max_attempts == 0 {
            return Err(Error::Value {
                key: String::from(if self.timeout_ms == 0 {
                    "terminology.timeout_ms"
                } else {
                    "terminology.retry.max_attempts"
                }),
                rule: "greater than zero",
            });
        }
        self.inline_secrets(profile)?;
        let mut config = Config::new(base, WireVersion::R4)
            .with_timeout(Duration::from_millis(self.timeout_ms))
            .with_retry(RetryPolicy {
                max_attempts: self.retry.max_attempts,
                initial_backoff: Duration::from_millis(self.retry.initial_backoff_ms),
                max_backoff: Duration::from_millis(self.retry.max_backoff_ms),
            });
        match self
            .credentials
            .resolve_with_tls(&ferrohealth_smart::config::ServerTls)
            .map_err(|source| Error::Credentials {
                key: "terminology.credentials",
                source: Box::new(source),
            })? {
            Some(Outbound::Basic(credential)) => {
                config = config.with_credentials(Credentials::Basic {
                    user: credential.username().to_owned(),
                    password: credential.password().clone(),
                });
            }
            Some(Outbound::Oauth(grant)) => {
                let source = TokenSource::new(*grant).map_err(|source| Error::Token {
                    key: "terminology.credentials.oauth",
                    source: Box::new(source),
                })?;
                config =
                    config.with_credentials(Credentials::OAuth(Arc::new(SmartToken::from(source))));
            }
            None => {}
        }
        let tls = TlsClient::read("terminology.tls", &self.tls).map_err(Error::TlsClient)?;
        let builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_millis(self.timeout_ms));
        let http = tls
            .apply(builder)
            .map_err(Error::TlsClient)?
            .build()
            .map_err(|source| Error::HttpClient {
                key: "terminology",
                source: Box::new(source),
            })?;
        let client = Client::with_http(config, http).map_err(|source| Error::Terminology {
            source: Box::new(source),
        })?;
        Ok(Some(TerminologySettings { client }))
    }

    /// Refuses a password or a client secret set inline under production,
    /// as [`crate::config::secret`] does for every other secret.
    fn inline_secrets(&self, profile: Profile) -> Result<(), Error> {
        if profile != Profile::Production {
            return Ok(());
        }
        let credentials = &self.credentials;
        if credentials.password.is_some() && credentials.password_file.is_none() {
            return Err(Error::InlineSecret {
                key: String::from("terminology.credentials.password"),
            });
        }
        if credentials.oauth.as_ref().is_some_and(|oauth| {
            oauth.client_secret.is_some() && oauth.client_secret_file.is_none()
        }) {
            return Err(Error::InlineSecret {
                key: String::from("terminology.credentials.oauth.client_secret"),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::Terminology;
    use crate::config::Profile;
    use crate::config::error::Error;

    fn at(base_url: &str) -> Terminology {
        Terminology {
            base_url: base_url.to_owned(),
            ..Terminology::default()
        }
    }

    #[test]
    fn no_base_url_is_no_terminology_server() {
        assert!(matches!(
            Terminology::default().resolve(Profile::Production),
            Ok(None)
        ));
    }

    #[test]
    fn a_base_url_resolves_to_a_client_of_it() {
        let resolved = at("https://tx.example.org/fhir/r4")
            .resolve(Profile::Production)
            .expect("resolves")
            .expect("on");
        assert_eq!(
            resolved.client.config().base_url.as_str(),
            "https://tx.example.org/fhir/r4"
        );
        assert!(
            at("http://localhost:8081/r4")
                .resolve(Profile::Production)
                .is_ok_and(|resolved| resolved.is_some()),
            "plain http on loopback"
        );
    }

    #[test]
    fn plain_http_off_loopback_userinfo_and_zero_bounds_are_refused() {
        for (section, key) in [
            (at("http://tx.example.org/r4"), "terminology.base_url"),
            (
                at("https://user:pw@tx.example.org/r4"),
                "terminology.base_url",
            ),
            (
                Terminology {
                    timeout_ms: 0,
                    ..at("https://tx.example.org/r4")
                },
                "terminology.timeout_ms",
            ),
        ] {
            assert!(
                matches!(section.resolve(Profile::Production), Err(Error::Value { key: refused, .. }) if refused == key),
                "{key}"
            );
        }
        assert!(matches!(
            at("not a url").resolve(Profile::Production),
            Err(Error::Url { .. })
        ));
    }

    #[test]
    fn an_inline_password_is_refused_under_production_only() {
        let mut section = at("https://tx.example.org/r4");
        section.credentials.user = String::from("ferropix");
        section.credentials.password = Some(SecretString::from("synthetic"));
        assert!(matches!(
            section.resolve(Profile::Production),
            Err(Error::InlineSecret { key }) if key == "terminology.credentials.password"
        ));
        assert!(
            section
                .resolve(Profile::Development)
                .is_ok_and(|resolved| resolved.is_some())
        );
    }

    #[test]
    fn credentials_ferrohealth_smart_refuses_are_named() {
        let mut section = at("https://tx.example.org/r4");
        section.credentials.password = Some(SecretString::from("synthetic"));
        assert!(matches!(
            section.resolve(Profile::Development),
            Err(Error::Credentials { key, .. }) if key == "terminology.credentials"
        ));
    }
}
