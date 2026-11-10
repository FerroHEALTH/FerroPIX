// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! One module per section of the configuration tree, each with the section
//! as written, its defaults and its resolution into settings.

pub mod audit;
pub mod auth;
pub mod db;
pub mod dev;
pub mod domain;
pub mod hl7v2;
pub mod matching;
pub mod operator;
pub mod outbox;
pub mod profiles;
pub mod pseudonym;
pub mod public;
pub mod terminology;
pub mod xcpd;

use std::net::{IpAddr, SocketAddr};

use ferrohealth_smart::config::credentials::CredentialsSection;

use crate::config::Profile;
use crate::config::error::Error;

/// Returns `text` as a socket address, refused under `key`.
pub(crate) fn socket_address(key: &str, text: &str) -> Result<SocketAddr, Error> {
    text.parse().map_err(|source| Error::Listen {
        key: key.to_owned(),
        source,
    })
}

/// Returns `text` as a URL, refused under `key`.
pub(crate) fn url(key: &str, text: &str) -> Result<url::Url, Error> {
    url::Url::parse(text).map_err(|source| Error::Url {
        key: key.to_owned(),
        source,
    })
}

/// Returns whether `url` names a loopback host: `localhost`, an IPv4 address
/// in `127.0.0.0/8` or `::1`.
pub(crate) fn is_loopback_url(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

/// Returns whether `address` binds a loopback address.
pub(crate) fn is_loopback_address(address: SocketAddr) -> bool {
    match address.ip() {
        IpAddr::V4(ip) => ip.is_loopback(),
        IpAddr::V6(ip) => ip.is_loopback(),
    }
}

/// Refuses a password or a client secret set inline in the `credentials`
/// of the endpoint table at `table` under production, as
/// [`crate::config::secret`] does for every other secret.
///
/// # Errors
/// Returns [`Error::InlineSecret`] naming the key.
pub(crate) fn endpoint_secrets(
    table: &str,
    credentials: &CredentialsSection,
    profile: Profile,
) -> Result<(), Error> {
    if profile != Profile::Production {
        return Ok(());
    }
    if credentials.password.is_some() && credentials.password_file.is_none() {
        return Err(Error::InlineSecret {
            key: format!("{table}.credentials.password"),
        });
    }
    if credentials
        .oauth
        .as_ref()
        .is_some_and(|oauth| oauth.client_secret.is_some() && oauth.client_secret_file.is_none())
    {
        return Err(Error::InlineSecret {
            key: format!("{table}.credentials.oauth.client_secret"),
        });
    }
    Ok(())
}
