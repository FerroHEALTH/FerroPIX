// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Localization Service search: `GET [base]/DocumentReference?
//! patient.identifier=…&type=…` (the `nl-gf-localization-repository`
//! capability statement, its `search-type` interaction and its
//! `patient.identifier` and `type` search parameters).

use std::fmt;

use secrecy::ExposeSecret;
use url::Url;

use super::error::{InvalidInput, Malformation};
use super::{LOINC_SYSTEM, PATIENT_DATA_TYPE};
use crate::identification::{PSEUDO_BSN_SYSTEM, PseudoBsn};

/// The resource type of a localization record, relative to the FHIR base.
const RESOURCE: &str = "DocumentReference";

/// The `[base]/DocumentReference` URL for the FHIR base `base` (FHIR R4 HTTP,
/// Service Base URL, <http://hl7.org/fhir/R4/http.html#root>).
pub(super) fn endpoint(mut base: Url) -> Result<Url, InvalidInput> {
    if !matches!(base.scheme(), "http" | "https")
        || base.cannot_be_a_base()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(InvalidInput::Base);
    }
    if !base.path().ends_with('/') {
        let path = format!("{}/", base.path());
        base.set_path(&path);
    }
    base.join(RESOURCE)
        .map_err(|_unjoinable| InvalidInput::Base)
}

/// The search URL: the patient by pseudonymised BSN and the one record type
/// the IG fixes.
///
/// The URL holds the pseudonym, so it is handed to the HTTP client and never
/// kept, logged or put into an error.
pub(super) fn query(endpoint: &Url, patient: &PseudoBsn) -> Url {
    let mut url = endpoint.clone();
    url.query_pairs_mut()
        .append_pair(
            "patient.identifier",
            &format!(
                "{}|{}",
                escape(PSEUDO_BSN_SYSTEM),
                escape(patient.value().expose_secret())
            ),
        )
        .append_pair(
            "type",
            &format!("{}|{}", escape(LOINC_SYSTEM), escape(PATIENT_DATA_TYPE)),
        );
    url
}

/// The request URL `url` without its query and fragment, the `htu` a `DPoP`
/// proof binds (RFC 9449 §4.2), which an authorizer is handed so the
/// pseudonym in the query never reaches it.
pub(super) fn target(url: &Url) -> Url {
    let mut target = url.clone();
    target.set_query(None);
    target.set_fragment(None);
    target
}

/// The `next` link `next` as a URL to follow, when it stays on the search
/// endpoint: the same scheme, host, port and path, so the pseudonym it may
/// carry reaches no other resource or server (FHIR R4 paging,
/// <http://hl7.org/fhir/R4/http.html#paging>; no specification governs the
/// rule: our own design).
pub(super) fn next(endpoint: &Url, next: &str) -> Result<Url, Malformation> {
    let url = endpoint
        .join(next)
        .map_err(|_unparsable| Malformation::NextLink)?;
    let same = url.scheme() == endpoint.scheme()
        && url.host_str() == endpoint.host_str()
        && url.port_or_known_default() == endpoint.port_or_known_default()
        && url.path() == endpoint.path()
        && url.username() == endpoint.username()
        && url.password() == endpoint.password()
        && url.fragment().is_none();
    if same {
        Ok(url)
    } else {
        Err(Malformation::NextLink)
    }
}

/// Returns `part` with the characters FHIR search gives a meaning to escaped
/// by a backslash: `\`, `|`, `,` and `$` (FHIR R4 search, Escaping Search
/// Parameters, <http://hl7.org/fhir/R4/search.html#escaping>).
fn escape(part: &str) -> String {
    let mut escaped = String::with_capacity(part.len());
    for character in part.chars() {
        if matches!(character, '\\' | '|' | ',' | '$') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// A URL whose `Debug` shows it without its userinfo and its query, either
/// of which may hold a credential.
pub(super) struct Redacted<'a>(pub(super) &'a Url);

impl fmt::Debug for Redacted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut shown = self.0.clone();
        let cleared = shown.set_username("").is_ok() && shown.set_password(None).is_ok();
        shown.set_query(None);
        if cleared {
            fmt::Debug::fmt(shown.as_str(), f)
        } else {
            f.write_str("\"***\"")
        }
    }
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use url::Url;

    use super::{Redacted, endpoint, escape, next, query};
    use crate::identification::PseudoBsn;
    use crate::nvi::error::{InvalidInput, Malformation};

    fn search() -> Url {
        endpoint(Url::parse("https://nvi.example.org/fhir").expect("a URL")).expect("an endpoint")
    }

    #[test]
    fn the_endpoint_is_the_resource_type_under_the_base() {
        for base in [
            "https://nvi.example.org/fhir",
            "https://nvi.example.org/fhir/",
        ] {
            let url = endpoint(Url::parse(base).expect("a URL")).expect("an endpoint");
            assert_eq!(
                url.as_str(),
                "https://nvi.example.org/fhir/DocumentReference"
            );
        }
        for base in [
            "https://nvi.example.org/fhir?x=1",
            "https://nvi.example.org/fhir#top",
            "ftp://nvi.example.org/fhir",
            "urn:oid:2.999.1",
        ] {
            assert_eq!(
                endpoint(Url::parse(base).expect("a URL")),
                Err(InvalidInput::Base),
                "{base}"
            );
        }
    }

    #[test]
    fn the_query_names_the_pseudonym_and_the_fixed_type() {
        let patient = PseudoBsn::new(SecretString::from("p|1")).expect("a pseudonym");
        let url = query(&search(), &patient);
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert_eq!(
            pairs,
            [
                (
                    "patient.identifier".to_owned(),
                    r"http://fhir.nl/fhir/NamingSystem/pseudo-bsn|p\|1".to_owned()
                ),
                ("type".to_owned(), "http://loinc.org|55188-7".to_owned()),
            ],
            "the value's own | is escaped, the separator is not"
        );
    }

    #[test]
    fn a_next_link_is_followed_only_on_the_search_endpoint() {
        let endpoint = search();
        for link in [
            "https://nvi.example.org/fhir/DocumentReference?_page=2",
            "DocumentReference?_page=2",
            "https://nvi.example.org:443/fhir/DocumentReference?_page=2",
        ] {
            assert!(next(&endpoint, link).is_ok(), "{link}");
        }
        for link in [
            "https://elsewhere.example.org/fhir/DocumentReference?_page=2",
            "http://nvi.example.org/fhir/DocumentReference?_page=2",
            "https://nvi.example.org/fhir/Patient?_page=2",
            "https://nvi.example.org:8443/fhir/DocumentReference?_page=2",
            "https://user@nvi.example.org/fhir/DocumentReference?_page=2",
            "https://nvi.example.org/fhir/DocumentReference#x",
        ] {
            assert_eq!(next(&endpoint, link), Err(Malformation::NextLink), "{link}");
        }
    }

    #[test]
    fn the_separators_fhir_search_defines_are_escaped() {
        assert_eq!(escape(r"a|b,c$d\e"), r"a\|b\,c\$d\\e");
    }

    #[test]
    fn a_rendered_url_shows_no_credential() {
        let url = Url::parse("https://user:secret@nvi.example.org/fhir/DocumentReference?key=k")
            .expect("a URL");
        let shown = format!("{:?}", Redacted(&url));
        assert_eq!(shown, "\"https://nvi.example.org/fhir/DocumentReference\"");
    }
}
