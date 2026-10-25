// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The authorization server's issuer identifier, as the Nuts grant holds its
//! metadata to it (RFC 8414 §2, §3.1 and §3.3).
//!
//! The rules are `ferrohealth_smart::metadata`'s: an issuer in the canonical
//! form the URL parser writes, without userinfo ([`canonical_issuer`]), the
//! well-known suffix inserted between the host and the path ([`locations`]),
//! the stated issuer compared as text ([`check_issuer`]) and every endpoint
//! on the issuer's origin ([`same_origin`]). The grant adds one refusal of
//! its own: an endpoint with userinfo or a fragment.

use ferrohealth_smart::metadata::{
    DiscoveryError, canonical_issuer, check_issuer, locations, same_origin,
};
use url::Url;

/// An authorization server's issuer identifier (RFC 8414 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Issuer {
    text: String,
    url: Url,
}

/// An issuer identifier the grant cannot use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InvalidIssuer;

/// Why an endpoint the metadata names is not one the grant sends to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EndpointError {
    /// The value is not a URL.
    NotAUrl,
    /// The endpoint is not on the issuer's origin (scheme, host and port).
    OtherOrigin,
    /// The endpoint carries userinfo or a fragment.
    Insecure,
}

impl Issuer {
    /// Reads `text` as an issuer identifier, a text [`canonical_issuer`]
    /// takes.
    pub(super) fn parse(text: &str) -> Result<Self, InvalidIssuer> {
        let url = canonical_issuer(text).map_err(|_refused| InvalidIssuer)?;
        Ok(Self {
            text: text.to_owned(),
            url,
        })
    }

    /// The issuer identifier, as written.
    pub(super) fn as_str(&self) -> &str {
        &self.text
    }

    /// The URL of the issuer's metadata, the RFC 8414 §3.1 location.
    pub(super) fn metadata_url(&self) -> Url {
        let [oauth, _openid] = locations(&self.url);
        oauth
    }

    /// Whether `named`, the `issuer` a metadata document states, is this
    /// issuer (RFC 8414 §3.3).
    ///
    /// The issuer is held to its canonical form, so the same text is also
    /// the same URL.
    pub(super) fn is_identical_to(&self, named: &str) -> bool {
        check_issuer(&self.text, named).is_ok()
    }

    /// The endpoint `text`, the value of the metadata member `member`, when
    /// the grant may send to it: a URL on the issuer's origin, which also
    /// holds it to the issuer's scheme, with no userinfo and no fragment.
    pub(super) fn endpoint(&self, member: &'static str, text: &str) -> Result<Url, EndpointError> {
        let url = same_origin(&self.url, member, text).map_err(|refused| match refused {
            DiscoveryError::OtherOrigin { .. } => EndpointError::OtherOrigin,
            _ => EndpointError::NotAUrl,
        })?;
        // NOTE: no specification governs this: our own design; an endpoint the
        // grant sends a credential to carries no userinfo and no fragment.
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err(EndpointError::Insecure);
        }
        Ok(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ISSUER: &str = "https://as.example.org/oauth2/hospital";

    fn issuer() -> Issuer {
        Issuer::parse(ISSUER).expect("issuer")
    }

    #[test]
    fn the_well_known_suffix_goes_between_host_and_path() {
        assert_eq!(
            issuer().metadata_url().as_str(),
            "https://as.example.org/.well-known/oauth-authorization-server/oauth2/hospital"
        );
        let bare = Issuer::parse("https://as.example.org").expect("issuer");
        assert_eq!(
            bare.metadata_url().as_str(),
            "https://as.example.org/.well-known/oauth-authorization-server"
        );
        let slash = Issuer::parse("https://as.example.org/tenant/").expect("issuer");
        assert_eq!(
            slash.metadata_url().as_str(),
            "https://as.example.org/.well-known/oauth-authorization-server/tenant"
        );
    }

    #[test]
    fn an_issuer_outside_rfc_8414_or_its_canonical_form_is_refused() {
        for text in [
            "not a url",
            "ftp://as.example.org",
            "http://as.example.org",
            "https://user@as.example.org",
            "https://as.example.org/?q=1",
            "https://as.example.org/#f",
            "https://AS.example.org/tenant",
            "https://as.example.org:443/tenant",
        ] {
            assert_eq!(Err(InvalidIssuer), Issuer::parse(text), "{text}");
        }
    }

    #[test]
    fn a_plain_http_issuer_is_taken_on_a_loopback_host_only() {
        assert!(Issuer::parse("http://127.0.0.1:8080/oauth2").is_ok());
        assert!(Issuer::parse("http://localhost:8080").is_ok());
        assert_eq!(
            Err(InvalidIssuer),
            Issuer::parse("http://as.example.org/oauth2")
        );
    }

    #[test]
    fn an_issuer_that_differs_in_text_or_url_is_not_identical() {
        assert!(issuer().is_identical_to(ISSUER));
        for named in [
            "https://as.example.org/oauth2/other",
            "https://AS.example.org/oauth2/hospital",
            "https://as.example.org:443/oauth2/hospital",
            "https://as.example.org/oauth2/hospital/",
        ] {
            assert!(!issuer().is_identical_to(named), "{named}");
        }
    }

    #[test]
    fn an_endpoint_on_the_issuers_origin_is_taken() {
        let url = issuer()
            .endpoint("token_endpoint", &format!("{ISSUER}/token"))
            .expect("taken");
        assert_eq!(url.as_str(), format!("{ISSUER}/token"));
    }

    #[test]
    fn an_endpoint_off_the_issuers_origin_is_refused() {
        for other in [
            "http://as.example.org/oauth2/hospital/token",
            "https://elsewhere.example.org/token",
            "https://as.example.org:8443/token",
            "https://as.example.org.evil.example/token",
        ] {
            assert_eq!(
                Err(EndpointError::OtherOrigin),
                issuer().endpoint("token_endpoint", other),
                "{other}"
            );
        }
    }

    #[test]
    fn an_endpoint_with_userinfo_or_a_fragment_is_refused() {
        for other in [
            "https://user@as.example.org/token",
            "https://as.example.org/token#f",
        ] {
            assert_eq!(
                Err(EndpointError::Insecure),
                issuer().endpoint("token_endpoint", other),
                "{other}"
            );
        }
        assert_eq!(
            Err(EndpointError::NotAUrl),
            issuer().endpoint("token_endpoint", "token")
        );
    }
}
