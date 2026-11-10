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
//! package. FerroPIX holds no copy of a table. The server is the family's
//! endpoint table, `[terminology.endpoint]` of `ferrohealth-smart`: the
//! base URL, the timeouts, the retry budget, the TLS and the credentials
//! (HTTP Basic or the client credentials grant), reached through its
//! outbound client, as FerroBRIDGE's `[terminology]` is. No specification
//! governs the section: our own design.

use ferrobridge_term::client::Client;
use ferrobridge_term::config::WireVersion;
use ferrohealth_config::removed::{Removals, Removed};
use ferrohealth_config::units::Duration as Span;
use ferrohealth_smart::config::ServerTls;
use ferrohealth_smart::config::endpoint::{EndpointSection, RetrySection};
use serde::{Deserialize, Serialize};

use crate::config::Profile;
use crate::config::error::Error;
use crate::config::retired;
use crate::config::section::{endpoint_secrets, is_loopback_url, url};

/// The dotted key of the server's endpoint table.
pub const ENDPOINT: &str = "terminology.endpoint";

/// The `[terminology]` section as written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Terminology {
    /// The FHIR R4 terminology server (`[terminology.endpoint]`): its
    /// service base as `url`, empty for none, the timeouts, the retry
    /// budget, the TLS and the credentials.
    pub endpoint: EndpointSection,
    /// Retired in v0.0.24 for `endpoint.url`.
    #[serde(skip_serializing)]
    pub base_url: Removed,
    /// Retired in v0.0.24 for `endpoint.timeout`.
    #[serde(skip_serializing)]
    pub timeout_ms: Removed,
    /// Retired in v0.0.24 for `endpoint.retry`.
    #[serde(skip_serializing)]
    pub retry: Removed,
    /// Retired in v0.0.24 for `endpoint.credentials`.
    #[serde(skip_serializing)]
    pub credentials: Removed,
    /// Retired in v0.0.24 for `endpoint.tls`.
    #[serde(skip_serializing)]
    pub tls: Removed,
}

impl Default for Terminology {
    fn default() -> Self {
        Self {
            endpoint: EndpointSection {
                retry: RetrySection {
                    max_backoff: Span::from_secs(2),
                    ..RetrySection::default()
                },
                ..EndpointSection::default()
            },
            base_url: Removed::default(),
            timeout_ms: Removed::default(),
            retry: Removed::default(),
            credentials: Removed::default(),
            tls: Removed::default(),
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
    /// Gathers the retired keys the sources set.
    pub fn removals(&self, found: &mut Removals) {
        found.check(self.base_url, &retired::terminology::BASE_URL);
        found.check(self.timeout_ms, &retired::terminology::TIMEOUT_MS);
        found.check(self.retry, &retired::terminology::RETRY);
        found.check(self.credentials, &retired::terminology::CREDENTIALS);
        found.check(self.tls, &retired::terminology::TLS);
    }

    /// Resolves the section: `None` while `endpoint.url` is empty.
    ///
    /// # Errors
    /// Returns [`Error::Url`] for a base URL that does not parse,
    /// [`Error::Value`] for a base URL that is neither `https` nor `http` on
    /// a loopback host or that carries userinfo, [`Error::InlineSecret`] for
    /// a password or a client secret set inline under production, and
    /// [`Error::Terminology`] when the client refuses the table, its TLS or
    /// its credential.
    pub fn resolve(&self, profile: Profile) -> Result<Option<TerminologySettings>, Error> {
        let text = self.endpoint.url.expose().trim();
        if text.is_empty() {
            return Ok(None);
        }
        let base = url("terminology.endpoint.url", text)?;
        // NOTE: FHIR R4 security.html leaves transport security to the deployment;
        // our own design: plain http only on loopback, as for every outbound URL.
        if !(base.scheme() == "https" || (base.scheme() == "http" && is_loopback_url(&base))) {
            return Err(Error::Value {
                key: String::from("terminology.endpoint.url"),
                rule: "https, or http on a loopback host",
            });
        }
        if !base.username().is_empty() || base.password().is_some() {
            return Err(Error::Value {
                key: String::from("terminology.endpoint.url"),
                rule: "no userinfo: credentials go in [terminology.endpoint.credentials]",
            });
        }
        endpoint_secrets(ENDPOINT, &self.endpoint.credentials, profile)?;
        let client = Client::from_endpoint(&self.endpoint, ENDPOINT, &ServerTls, WireVersion::R4)
            .map_err(|source| Error::Terminology {
            source: Box::new(source),
        })?;
        Ok(Some(TerminologySettings { client }))
    }
}

#[cfg(test)]
mod tests {
    use ferrohealth_config::units::Duration as Span;
    use ferrohealth_secret::secret_url::SecretUrl;
    use secrecy::SecretString;

    use super::Terminology;
    use crate::config::Profile;
    use crate::config::error::Error;

    fn at(url: &str) -> Terminology {
        let mut section = Terminology::default();
        section.endpoint.url = SecretUrl::new(url);
        section
    }

    #[test]
    fn no_url_is_no_terminology_server() {
        assert!(matches!(
            Terminology::default().resolve(Profile::Production),
            Ok(None)
        ));
    }

    #[test]
    fn a_url_resolves_to_a_client_of_it() {
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
    fn plain_http_off_loopback_and_userinfo_are_refused() {
        for section in [
            at("http://tx.example.org/r4"),
            at("https://user:pw@tx.example.org/r4"),
        ] {
            assert!(
                matches!(section.resolve(Profile::Production), Err(Error::Value { key, .. }) if key == "terminology.endpoint.url"),
                "{section:?}"
            );
        }
        assert!(matches!(
            at("not a url").resolve(Profile::Production),
            Err(Error::Url { .. })
        ));
    }

    #[test]
    fn a_zero_timeout_is_refused_by_the_endpoint_table() {
        let mut section = at("https://tx.example.org/r4");
        section.endpoint.timeout = Span::from_secs(0);
        assert!(matches!(
            section.resolve(Profile::Production),
            Err(Error::Terminology { .. })
        ));
    }

    #[test]
    fn an_inline_password_is_refused_under_production_only() {
        let mut section = at("https://tx.example.org/r4");
        section.endpoint.credentials.user = String::from("ferropix");
        section.endpoint.credentials.password = Some(SecretString::from("synthetic"));
        assert!(matches!(
            section.resolve(Profile::Production),
            Err(Error::InlineSecret { key }) if key == "terminology.endpoint.credentials.password"
        ));
        assert!(
            section
                .resolve(Profile::Development)
                .is_ok_and(|resolved| resolved.is_some())
        );
    }

    #[test]
    fn credentials_ferrohealth_smart_refuses_are_refused() {
        let mut section = at("https://tx.example.org/r4");
        section.endpoint.credentials.password = Some(SecretString::from("synthetic"));
        assert!(matches!(
            section.resolve(Profile::Development),
            Err(Error::Terminology { .. })
        ));
    }
}
