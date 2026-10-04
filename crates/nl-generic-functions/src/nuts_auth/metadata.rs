// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The authorization server metadata the grant reads (RFC 8414; Nuts RFC021
//! §3.1 and §5; RFC 9449 §5.1).

use std::collections::BTreeMap;

use serde::Deserialize;
use url::Url;

use crate::nuts_auth::error::MetadataError;
use crate::nuts_auth::presentation::JWT_VP;

/// The well-known URI suffix of authorization server metadata (RFC 8414
/// §3, §7.3).
pub(super) const WELL_KNOWN: &str = "/.well-known/oauth-authorization-server";

/// The metadata members the grant reads.
#[derive(Deserialize)]
pub(super) struct Raw {
    issuer: Option<String>,
    token_endpoint: Option<String>,
    presentation_definition_endpoint: Option<String>,
    vp_formats: Option<BTreeMap<String, Format>>,
    dpop_signing_alg_values_supported: Option<Vec<String>>,
}

/// One entry of `vp_formats`: the algorithms it admits, under the name
/// Presentation Exchange 2.0.0 §Claim Format Designations gives them
/// (`alg`) or under `alg_values_supported`.
#[derive(Deserialize)]
struct Format {
    alg: Option<Vec<String>>,
    alg_values_supported: Option<Vec<String>>,
}

/// What the grant takes from the metadata.
#[derive(Debug, Clone)]
pub(super) struct Metadata {
    pub(super) token_endpoint: Url,
    pub(super) definition_endpoint: Url,
}

/// The metadata URL of the issuer `issuer`: the well-known suffix inserted
/// between its host and its path, with a terminating `/` of the path removed
/// (RFC 8414 §3.1).
pub(super) fn url(issuer: &Url) -> Url {
    let mut url = issuer.clone();
    let path = issuer.path().trim_end_matches('/');
    url.set_path(&format!("{WELL_KNOWN}{path}"));
    url
}

/// Reads `raw`, the metadata of the issuer written `text` and parsed as
/// `url`, for a holder signing with `holder_algorithm` and a prover signing
/// with `proof_algorithm`.
///
/// The metadata's `issuer` must be the same text and parse to the same URL
/// (RFC 8414 §3.3), and every endpoint the grant sends to must be on the
/// issuer's origin, without userinfo or a fragment.
pub(super) fn read(
    raw: &Raw,
    (text, url): (&str, &Url),
    holder_algorithm: &str,
    proof_algorithm: &str,
) -> Result<Metadata, MetadataError> {
    let named = raw.issuer.as_deref().ok_or(MetadataError::Issuer)?;
    // NOTE: RFC 8414 §3.3, the issuer is identical to the one asked of: the same
    // text and, read by the parser every request URL goes through, the same URL.
    if named != text || Url::parse(named).ok().as_ref() != Some(url) {
        return Err(MetadataError::Issuer);
    }
    let token_endpoint = endpoint(raw.token_endpoint.as_deref(), url)
        .map_err(|refused| refused.unwrap_or(MetadataError::TokenEndpoint))?;
    let definition_endpoint = endpoint(raw.presentation_definition_endpoint.as_deref(), url)
        .map_err(|refused| refused.unwrap_or(MetadataError::DefinitionEndpoint))?;
    let admits = raw
        .vp_formats
        .as_ref()
        .and_then(|formats| formats.get(JWT_VP))
        .is_some_and(|format| match (&format.alg, &format.alg_values_supported) {
            (None, None) => true,
            (alg, values) => alg
                .iter()
                .chain(values.iter())
                .flatten()
                .any(|alg| alg == holder_algorithm),
        });
    if !admits {
        return Err(MetadataError::PresentationFormat {
            algorithm: holder_algorithm.to_owned(),
        });
    }
    if let Some(supported) = &raw.dpop_signing_alg_values_supported
        && !supported.iter().any(|alg| alg == proof_algorithm)
    {
        return Err(MetadataError::ProofAlgorithm {
            algorithm: proof_algorithm.to_owned(),
        });
    }
    Ok(Metadata {
        token_endpoint,
        definition_endpoint,
    })
}

/// The endpoint `text` names, or `Err(None)` when it is absent or no URL and
/// `Err(Some(_))` when it is a URL the grant does not send to: off the origin
/// of `issuer`, which also holds it to the issuer's scheme, or with userinfo
/// or a fragment.
fn endpoint(text: Option<&str>, issuer: &Url) -> Result<Url, Option<MetadataError>> {
    let url = text.and_then(|text| Url::parse(text).ok()).ok_or(None)?;
    // NOTE: no specification governs this: our own design; RFC 8414 and RFC021 place
    // no endpoint, and the holder's credentials go to the issuer's origin alone.
    if url.origin() != issuer.origin() {
        return Err(Some(MetadataError::OtherOrigin));
    }
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(Some(MetadataError::InsecureEndpoint));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ISSUER: &str = "https://as.example.org/oauth2/hospital";

    fn raw(issuer: &str, token_endpoint: &str) -> Raw {
        serde_json::from_str(&format!(
            r#"{{"issuer": "{issuer}", "token_endpoint": "{token_endpoint}",
                "presentation_definition_endpoint": "{ISSUER}/presentation_definition",
                "vp_formats": {{"jwt_vp": {{"alg": ["ES256"]}}}}}}"#
        ))
        .expect("metadata")
    }

    fn read_as_issuer(raw: &Raw) -> Result<Metadata, MetadataError> {
        let url = Url::parse(ISSUER).expect("url");
        read(raw, (ISSUER, &url), "ES256", "ES256")
    }

    #[test]
    fn the_well_known_suffix_goes_between_host_and_path() {
        let issuer = Url::parse(ISSUER).expect("url");
        assert_eq!(
            url(&issuer).as_str(),
            "https://as.example.org/.well-known/oauth-authorization-server/oauth2/hospital"
        );
        let bare = Url::parse("https://as.example.org").expect("url");
        assert_eq!(
            url(&bare).as_str(),
            "https://as.example.org/.well-known/oauth-authorization-server"
        );
        let slash = Url::parse("https://as.example.org/tenant/").expect("url");
        assert_eq!(
            url(&slash).as_str(),
            "https://as.example.org/.well-known/oauth-authorization-server/tenant"
        );
    }

    #[test]
    fn an_endpoint_on_the_issuers_origin_is_taken() {
        let metadata = read_as_issuer(&raw(ISSUER, &format!("{ISSUER}/token"))).expect("taken");
        assert_eq!(metadata.token_endpoint.as_str(), format!("{ISSUER}/token"));
    }

    #[test]
    fn an_issuer_that_differs_in_text_or_url_is_refused() {
        for named in [
            "https://as.example.org/oauth2/other",
            "https://AS.example.org/oauth2/hospital",
            "https://as.example.org:443/oauth2/hospital",
            "https://as.example.org/oauth2/hospital/",
        ] {
            assert!(
                matches!(
                    read_as_issuer(&raw(named, &format!("{ISSUER}/token"))),
                    Err(MetadataError::Issuer)
                ),
                "{named}"
            );
        }
    }

    #[test]
    fn a_plain_http_token_endpoint_under_an_https_issuer_is_refused() {
        let refused = read_as_issuer(&raw(ISSUER, "http://as.example.org/oauth2/hospital/token"));
        assert!(
            matches!(refused, Err(MetadataError::OtherOrigin)),
            "{refused:?}"
        );
    }

    #[test]
    fn a_token_endpoint_on_another_origin_is_refused() {
        for other in [
            "https://elsewhere.example.org/token",
            "https://as.example.org:8443/token",
            "https://as.example.org.evil.example/token",
        ] {
            let refused = read_as_issuer(&raw(ISSUER, other));
            assert!(
                matches!(refused, Err(MetadataError::OtherOrigin)),
                "{other}"
            );
        }
    }

    #[test]
    fn a_token_endpoint_with_userinfo_or_a_fragment_is_refused() {
        for other in [
            "https://user@as.example.org/token",
            "https://as.example.org/token#f",
        ] {
            let refused = read_as_issuer(&raw(ISSUER, other));
            assert!(
                matches!(refused, Err(MetadataError::InsecureEndpoint)),
                "{other}"
            );
        }
    }

    #[test]
    fn a_non_string_endpoint_is_refused_by_the_reader() {
        let parsed = serde_json::from_str::<Raw>(
            r#"{"issuer": "https://as.example.org", "token_endpoint": 7}"#,
        );
        assert!(parsed.is_err());
    }
}
