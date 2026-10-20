// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The authorization server's issuer identifier, as the Nuts grant holds its
//! metadata to it (RFC 8414 §2, §3.1 and §3.3).
//!
//! The URL rule and the metadata location are `ferrohealth_smart::metadata`'s:
//! an `https` URL without a query or a fragment, plain `http` only on a
//! loopback host ([`issuer_url`]), and the well-known suffix inserted between
//! the host and the path ([`locations`]). The grant adds what it compares as
//! text: the issuer is written in the canonical form the URL parser gives it,
//! without userinfo, so the text compared and the URL fetched have one
//! meaning; every endpoint it sends to is on the issuer's origin; and an
//! answer whose objects repeat a name is refused before it is read.

use std::collections::BTreeSet;
use std::fmt;

use ferrohealth_smart::metadata::{issuer_url, locations};
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
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
    /// Reads `text` as an issuer identifier: a URL [`issuer_url`] takes,
    /// without userinfo, written in the canonical form the URL parser gives
    /// it (a lower-case host, no default port).
    pub(super) fn parse(text: &str) -> Result<Self, InvalidIssuer> {
        let url = issuer_url(text).map_err(|_refused| InvalidIssuer)?;
        if !url.username().is_empty() || url.password().is_some() || !canonical(text, &url) {
            return Err(InvalidIssuer);
        }
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
    /// issuer: the same text and, read by the parser every request URL goes
    /// through, the same URL (RFC 8414 §3.3).
    pub(super) fn is_identical_to(&self, named: &str) -> bool {
        // NOTE: RFC 8414 §3.3, a text the URL parser cannot read is not this
        // issuer, so the parse failure is the answer.
        named == self.text && Url::parse(named).ok().as_ref() == Some(&self.url)
    }

    /// The endpoint `text` names, when the grant may send to it: a URL on the
    /// issuer's origin, which also holds it to the issuer's scheme, with no
    /// userinfo and no fragment.
    pub(super) fn endpoint(&self, text: &str) -> Result<Url, EndpointError> {
        let url = Url::parse(text).map_err(|_unparsable| EndpointError::NotAUrl)?;
        // NOTE: no specification governs this: our own design; RFC 8414 places no
        // endpoint, and what the grant sends goes to the issuer's origin alone.
        if url.origin() != self.url.origin() {
            return Err(EndpointError::OtherOrigin);
        }
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err(EndpointError::Insecure);
        }
        Ok(url)
    }
}

/// Whether `text` is the serialization of `url`, the form the URL parser
/// writes, or that form less the `/` it gives an empty path.
///
/// An issuer is compared as text (RFC 8414 §3.3) and fetched as a URL, so
/// only a text the parser leaves unchanged has one meaning for both (no
/// specification governs this: our own design).
fn canonical(text: &str, url: &Url) -> bool {
    text == url.as_str() || (url.path() == "/" && url.as_str().strip_suffix('/') == Some(text))
}

/// Proves that no object of the JSON text `body` repeats a name, at any
/// depth.
///
/// RFC 8259 §4 leaves the meaning of an object with a repeated name to each
/// parser, so two readers of one answer can take different values from it:
/// an endpoint the grant checks and one it sends to. An answer that repeats
/// a name is refused instead (no specification governs this: our own
/// design).
pub(super) fn repeats_no_name(body: &[u8]) -> Result<(), serde_json::Error> {
    serde_json::from_slice::<Unique>(body).map(|Unique| ())
}

/// A JSON value read only to prove that no object in it repeats a name.
struct Unique;

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(UniqueVisitor)
    }
}

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = Unique;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value whose objects repeat no name")
    }

    fn visit_bool<E: de::Error>(self, _value: bool) -> Result<Unique, E> {
        Ok(Unique)
    }

    fn visit_i64<E: de::Error>(self, _value: i64) -> Result<Unique, E> {
        Ok(Unique)
    }

    fn visit_u64<E: de::Error>(self, _value: u64) -> Result<Unique, E> {
        Ok(Unique)
    }

    fn visit_f64<E: de::Error>(self, _value: f64) -> Result<Unique, E> {
        Ok(Unique)
    }

    fn visit_str<E: de::Error>(self, _value: &str) -> Result<Unique, E> {
        Ok(Unique)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
        Ok(Unique)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Unique, A::Error> {
        while seq.next_element::<Unique>()?.is_some() {}
        Ok(Unique)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Unique, A::Error> {
        let mut names = BTreeSet::new();
        while let Some(name) = map.next_key::<String>()? {
            if !names.insert(name) {
                return Err(de::Error::custom("an object repeats a name"));
            }
            map.next_value::<Unique>()?;
        }
        Ok(Unique)
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
            .endpoint(&format!("{ISSUER}/token"))
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
                issuer().endpoint(other),
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
                issuer().endpoint(other),
                "{other}"
            );
        }
        assert_eq!(Err(EndpointError::NotAUrl), issuer().endpoint("token"));
    }

    #[test]
    fn a_repeated_name_at_any_depth_is_refused() {
        assert!(repeats_no_name(br#"{"a": 1, "b": {"c": [1, {"d": 2}]}}"#).is_ok());
        assert!(repeats_no_name(br#"{"a": 1, "a": 2}"#).is_err());
        assert!(repeats_no_name(br#"{"a": {"b": 1, "b": 1}}"#).is_err());
        assert!(repeats_no_name(br#"[{"b": 1, "b": 1}]"#).is_err());
    }
}
