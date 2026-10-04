// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Authorization server metadata (RFC 8414), as both authentication tracks
//! of Annex B read it (feature `oauth-metadata`).
//!
//! The Nuts profile of Annex B §B.4 and the BgZ/eOverdracht track of Annex B
//! §B.4a both discover their authorization server from its issuer
//! identifier, and both hold the metadata to it before sending it anything:
//!
//! - an [`Issuer`] is an `http` or `https` URL without userinfo, query or
//!   fragment (RFC 8414 §2), written in the canonical form the URL parser
//!   gives it, so the text compared and the URL fetched have one meaning;
//! - its metadata is read from [`Issuer::metadata_url`], the well-known
//!   suffix inserted between its host and its path (RFC 8414 §3.1);
//! - the metadata's `issuer` must be identical to it (RFC 8414 §3.3),
//!   [`Issuer::is_identical_to`];
//! - every endpoint the client sends to must be on the issuer's origin,
//!   without userinfo or a fragment, [`Issuer::endpoint`];
//! - an answer whose objects repeat a name at any depth is refused before it
//!   is read, [`repeats_no_name`].
//!
//! Which members a track reads beyond these, and what it requires of them, is
//! the track's own: this module reads no member but through the caller.
//!
//! ```
//! use nl_generic_functions::oauth_metadata::{EndpointError, Issuer};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let issuer = Issuer::parse("https://as.example.org/tenant")?;
//! let metadata = "https://as.example.org/.well-known/oauth-authorization-server/tenant";
//! assert_eq!(metadata, issuer.metadata_url().as_str());
//! assert!(issuer.is_identical_to("https://as.example.org/tenant"));
//! let elsewhere = issuer.endpoint("https://elsewhere.example.org/token");
//! assert_eq!(Err(EndpointError::OtherOrigin), elsewhere);
//! # Ok(())
//! # }
//! ```

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use url::Url;

/// The well-known URI suffix of authorization server metadata (RFC 8414
/// §3, §7.3).
pub const WELL_KNOWN: &str = "/.well-known/oauth-authorization-server";

/// An authorization server's issuer identifier (RFC 8414 §2).
///
/// The issuer is an `https` URL (RFC 8414 §2); `http` is accepted for a test
/// or development setup, and the caller decides whether a deployment may use
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issuer {
    text: String,
    url: Url,
}

/// An issuer identifier that cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "the issuer is not an http(s) URL without userinfo, query or fragment (RFC 8414 §2), in canonical form"
)]
pub struct InvalidIssuer;

/// Why an endpoint the metadata names is not one the client sends to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EndpointError {
    /// The value is not a URL.
    #[error("the endpoint is not a URL")]
    NotAUrl,
    /// The endpoint is not on the issuer's origin (scheme, host and port).
    #[error("the endpoint is not on the issuer's origin")]
    OtherOrigin,
    /// The endpoint carries userinfo or a fragment.
    #[error("the endpoint carries userinfo or a fragment")]
    Insecure,
}

impl Issuer {
    /// Reads `text` as an issuer identifier (RFC 8414 §2).
    ///
    /// # Errors
    ///
    /// Returns [`InvalidIssuer`] for text that is no `http` or `https` URL,
    /// carries userinfo, a query or a fragment, or is not written in the
    /// canonical form the URL parser gives it (a lower-case host, no default
    /// port).
    pub fn parse(text: &str) -> Result<Self, InvalidIssuer> {
        let url = Url::parse(text).map_err(|_unparsable| InvalidIssuer)?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !canonical(text, &url)
        {
            return Err(InvalidIssuer);
        }
        Ok(Self {
            text: text.to_owned(),
            url,
        })
    }

    /// The issuer identifier, as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The issuer identifier, as the URL parser reads it.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The URL of the issuer's metadata: the well-known suffix inserted
    /// between its host and its path, with a terminating `/` of the path
    /// removed (RFC 8414 §3.1).
    #[must_use]
    pub fn metadata_url(&self) -> Url {
        let mut url = self.url.clone();
        let path = self.url.path().trim_end_matches('/');
        url.set_path(&format!("{WELL_KNOWN}{path}"));
        url
    }

    /// Whether `named`, the `issuer` a metadata document states, is this
    /// issuer: the same text and, read by the parser every request URL goes
    /// through, the same URL (RFC 8414 §3.3).
    #[must_use]
    pub fn is_identical_to(&self, named: &str) -> bool {
        named == self.text && Url::parse(named).ok().as_ref() == Some(&self.url)
    }

    /// The endpoint `text` names, when the client may send to it: a URL on
    /// the issuer's origin, which also holds it to the issuer's scheme, with
    /// no userinfo and no fragment.
    ///
    /// # Errors
    ///
    /// Returns [`EndpointError::NotAUrl`] for text that is no URL,
    /// [`EndpointError::OtherOrigin`] for one off the issuer's origin, and
    /// [`EndpointError::Insecure`] for one with userinfo or a fragment.
    pub fn endpoint(&self, text: &str) -> Result<Url, EndpointError> {
        let url = Url::parse(text).map_err(|_unparsable| EndpointError::NotAUrl)?;
        // NOTE: no specification governs this: our own design; RFC 8414 places no
        // endpoint, and what the client sends goes to the issuer's origin alone.
        if url.origin() != self.url.origin() {
            return Err(EndpointError::OtherOrigin);
        }
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err(EndpointError::Insecure);
        }
        Ok(url)
    }
}

impl fmt::Display for Issuer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
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
/// an endpoint the client checks and one it sends to. An answer that repeats
/// a name is refused instead (no specification governs this: our own
/// design).
///
/// # Errors
///
/// Returns the reader's error for text that is no JSON, and a data error at
/// the repeated name for one that repeats a name.
pub fn repeats_no_name(body: &[u8]) -> Result<(), serde_json::Error> {
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
