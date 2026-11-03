// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The checked settings the console runs on, resolved from the written
//! [`Config`] with every `_file` secret read.

use std::net::SocketAddr;
use std::time::Duration;

use ferrohealth_session::config::{OidcSection, SessionSettings};
use ferrohealth_smart::oidc::{OFFLINE_ACCESS, OPENID};
use ferrohealth_ui::i18n::Locale;
use url::Url;

use crate::config::Config;
use crate::config::error::Error;

/// The resolved configuration.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The socket address the console listens on.
    pub listen: SocketAddr,
    /// The locale a page renders in when the request names none the
    /// console's catalog holds.
    pub locale: Locale,
    /// The FerroPIX server the console is a client of.
    pub pix: PixSettings,
    /// The store and the lifetimes of the sign-in sessions.
    pub session: SessionSettings,
    /// The OpenID Provider operators sign in with, when one is configured.
    pub oidc: Option<OidcSettings>,
}

/// The resolved `[pix]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixSettings {
    /// The root of the FerroPIX server's HTTP surface, an `http` or `https`
    /// URL whose path ends in `/`, so a relative route joins beneath it.
    pub base: Url,
    /// How long one call to the server may take.
    pub timeout: Duration,
    /// The scope of an administrator at the server.
    pub admin_scope: String,
}

/// The resolved `[oidc]`: the section `ferrohealth-session` checked, and
/// what the console reads from it for its own pages.
#[derive(Debug, Clone)]
pub struct OidcSettings {
    /// The checked section, which builds the console's relying party.
    pub checked: ferrohealth_session::config::OidcSettings,
    /// The console's redirection endpoint, whose origin is the console's.
    redirect_uri: Url,
    /// Whether the section names the authorization, token and key-set
    /// endpoints, so the relying party is built with no discovery.
    endpoints_given: bool,
}

impl OidcSettings {
    /// The console's own origin, `scheme://host:port`: the origin of its
    /// redirection endpoint, which an unsafe request must come from.
    #[must_use]
    pub fn origin(&self) -> String {
        self.redirect_uri.origin().ascii_serialization()
    }

    /// The console's redirection endpoint.
    #[must_use]
    pub const fn redirect_uri(&self) -> &Url {
        &self.redirect_uri
    }

    /// Whether the relying party is built from the written endpoints alone,
    /// with no call to the provider.
    #[must_use]
    pub const fn endpoints_given(&self) -> bool {
        self.endpoints_given
    }
}

impl Config {
    /// Checks every value and reads every `_file` secret.
    ///
    /// # Errors
    /// Returns [`Error::Address`] for a listen address that does not parse,
    /// [`Error::Locale`] for a `server.locale` that is no language tag,
    /// [`Error::Missing`], [`Error::Url`] and [`Error::UrlShape`] for a URL
    /// that is absent, does not parse or is not one its key admits,
    /// [`Error::Zero`] for a zero timeout, lifetime or pool bound,
    /// [`Error::Scope`] for `offline_access` listed in `oidc.scopes`,
    /// [`Error::InsecureCookie`] for `session.secure_cookie = false` on a
    /// console that is not plain HTTP on loopback (its `oidc.redirect_uri`,
    /// or its `server.listen` address when it signs in nowhere), the secret
    /// errors of `oidc.client_secret` and `session.key`, and
    /// [`Error::Session`] for every other refusal of `[oidc]` and
    /// `[session]`.
    pub fn resolve(&self) -> Result<Settings, Error> {
        let listen: SocketAddr = self
            .server
            .listen
            .parse()
            .map_err(|source| Error::Address {
                key: String::from("server.listen"),
                source,
            })?;
        let locale = Locale::parse(&self.server.locale).map_err(|source| Error::Locale {
            key: String::from("server.locale"),
            source,
        })?;
        let base = base_url("pix.base_url", &self.pix.base_url)?;
        let timeout = positive("pix.timeout_ms", self.pix.timeout_ms).map(Duration::from_millis)?;
        let oidc = self.oidc.as_ref().map(resolve_oidc).transpose()?;
        let session = match &oidc {
            Some(oidc) => self.session.check(&oidc.checked)?,
            None => self.session.check_with(false)?,
        };
        // NOTE: RFC 6265bis §4.1.2.5: a cookie without `Secure` travels over plain
        // HTTP, so it is admitted only where the console itself is HTTP on loopback.
        let plain_on_loopback = oidc.as_ref().map_or_else(
            || listen.ip().is_loopback(),
            |oidc| oidc.redirect_uri.scheme() == "http" && is_loopback(&oidc.redirect_uri),
        );
        if !session.secure_cookie() && !plain_on_loopback {
            return Err(Error::InsecureCookie {
                key: String::from("session.secure_cookie"),
            });
        }
        Ok(Settings {
            listen,
            locale,
            pix: PixSettings {
                base,
                timeout,
                admin_scope: self.pix.admin_scope.clone(),
            },
            session,
            oidc,
        })
    }
}

/// Resolves `[oidc]`: the console's own rules for its provider and its
/// redirection endpoint, then the section's check, which reads its client
/// secret.
fn resolve_oidc(oidc: &OidcSection) -> Result<OidcSettings, Error> {
    provider_url("oidc.issuer", &oidc.issuer)?;
    let endpoints = [
        ("oidc.authorization_endpoint", &oidc.authorization_endpoint),
        ("oidc.token_endpoint", &oidc.token_endpoint),
        ("oidc.jwks_uri", &oidc.jwks_uri),
        ("oidc.end_session_endpoint", &oidc.end_session_endpoint),
        ("oidc.revocation_endpoint", &oidc.revocation_endpoint),
    ];
    for (key, endpoint) in endpoints {
        if let Some(text) = endpoint {
            provider_url(key, text)?;
        }
    }
    let redirect_uri = web_url("oidc.redirect_uri", &oidc.redirect_uri)?;
    // NOTE: RFC 6749 §3.1.2: the redirection endpoint URI MUST NOT include a
    // fragment component.
    if redirect_uri.fragment().is_some() {
        return Err(Error::UrlShape {
            key: String::from("oidc.redirect_uri"),
            reason: "must not carry a fragment",
        });
    }
    if oidc.client_id.trim().is_empty() {
        return Err(Error::Missing {
            key: String::from("oidc.client_id"),
        });
    }
    // NOTE: OpenID Connect Core 1.0 §3.1.2.1: an OpenID Connect request MUST
    // carry the `openid` scope value.
    if !oidc.scopes.iter().any(|scope| scope == OPENID) {
        return Err(Error::Missing {
            key: String::from("oidc.scopes (openid)"),
        });
    }
    // NOTE: OpenID Connect Core 1.0 §11: `offline_access` goes with `prompt`
    // `consent`, which the console sends only for `oidc.offline_access`.
    if oidc.scopes.iter().any(|scope| scope == OFFLINE_ACCESS) {
        return Err(Error::Scope {
            key: String::from("oidc.scopes"),
            reason: "must not list offline_access; set oidc.offline_access = true",
        });
    }
    if let Some(text) = &oidc.post_logout_redirect_uri {
        web_url("oidc.post_logout_redirect_uri", text)?;
    }
    let endpoints_given = oidc.authorization_endpoint.is_some()
        && oidc.token_endpoint.is_some()
        && oidc.jwks_uri.is_some();
    // NOTE: OpenID Connect RP-Initiated Logout 1.0 §3: the post-logout redirect is sent to
    // the end-session endpoint, so one the written endpoints lack is refused before discovery.
    if oidc.post_logout_redirect_uri.is_some()
        && endpoints_given
        && oidc.end_session_endpoint.is_none()
    {
        return Err(Error::Missing {
            key: String::from("oidc.end_session_endpoint (for post_logout_redirect_uri)"),
        });
    }
    Ok(OidcSettings {
        checked: oidc.check()?,
        redirect_uri,
        endpoints_given,
    })
}

/// Returns `value`, refusing zero.
fn positive(key: &str, value: u64) -> Result<u64, Error> {
    if value == 0 {
        Err(Error::Zero {
            key: key.to_owned(),
        })
    } else {
        Ok(value)
    }
}

/// Parses an `http` or `https` URL with a host and no userinfo.
fn web_url(key: &str, text: &str) -> Result<Url, Error> {
    if text.trim().is_empty() {
        return Err(Error::Missing {
            key: key.to_owned(),
        });
    }
    let url = Url::parse(text).map_err(|source| Error::Url {
        key: key.to_owned(),
        source,
    })?;
    let shape = |reason| Error::UrlShape {
        key: key.to_owned(),
        reason,
    };
    if !matches!(url.scheme(), "http" | "https") {
        return Err(shape("must be an http or https URL"));
    }
    if url.host().is_none() {
        return Err(shape("must name a host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(shape("must not carry userinfo"));
    }
    Ok(url)
}

/// Parses an OpenID Provider URL: `https`, or `http` on a loopback host.
fn provider_url(key: &str, text: &str) -> Result<Url, Error> {
    let url = web_url(key, text)?;
    // NOTE: RFC 6749 §3.1: the authorization server MUST require TLS at its
    // authorization endpoint, so plain `http` is admitted only on loopback.
    if url.scheme() == "https" || is_loopback(&url) {
        Ok(url)
    } else {
        Err(Error::UrlShape {
            key: key.to_owned(),
            reason: "must be an https URL unless its host is loopback",
        })
    }
}

/// Parses an `http` or `https` URL with a host, no userinfo, no query and no
/// fragment, and ends its path in `/`.
fn base_url(key: &str, text: &str) -> Result<Url, Error> {
    let mut url = web_url(key, text)?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err(Error::UrlShape {
            key: key.to_owned(),
            reason: "must not carry a query or a fragment",
        });
    }
    // NOTE: RFC 3986 §5.2.3: a reference merges with the base path up to its
    // last `/`, so the base ends in one or its last segment would be dropped.
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

/// Whether `url` names a loopback host: `localhost`, `127.0.0.0/8` or `::1`.
fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(name)) => name == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}
