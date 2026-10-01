// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-83 request: `GET [base]/Patient/$ihe-pix?sourceIdentifier=…
//! {&targetSystem=…}` (§2:3.83.4.1.2).

use secrecy::ExposeSecret;
use url::Url;

use super::error::InvalidInput;
use super::identifier::{SourceIdentifier, TargetSystem};

/// The `$ihe-pix` operation on the Manager's `Patient` type, relative to the
/// FHIR base (the `OperationDefinition`: `resource` `Patient`, `type` level).
const OPERATION: &str = "Patient/$ihe-pix";

/// The `[base]/Patient/$ihe-pix` URL for the FHIR base `base`.
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
    base.join(OPERATION)
        .map_err(|_unjoinable| InvalidInput::Base)
}

/// The request URL: one `sourceIdentifier` and one `targetSystem` per domain
/// asked about (§2:3.83.4.1.2.1, §2:3.83.4.1.2.2).
///
/// The URL holds the source identifier, so it is handed to the HTTP client and
/// never kept, logged or put into an error.
pub(super) fn query(endpoint: &Url, source: &SourceIdentifier, targets: &[TargetSystem]) -> Url {
    let mut url = endpoint.clone();
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair(
            "sourceIdentifier",
            &format!(
                "{}|{}",
                escape(source.system()),
                escape(source.value().expose_secret())
            ),
        );
        for target in targets {
            pairs.append_pair("targetSystem", &escape(target.as_str()));
        }
    }
    url
}

/// `part` with the characters FHIR search gives a meaning to escaped by a
/// backslash: `\`, `|`, `,` and `$` (FHIR R4 search, Escaping Search
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

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use url::Url;

    use super::{endpoint, escape, query};
    use crate::pixm::error::InvalidInput;
    use crate::pixm::identifier::{SourceIdentifier, TargetSystem};

    #[test]
    fn the_endpoint_is_the_type_level_operation_under_the_base() {
        for base in [
            "https://pix.example.org/fhir",
            "https://pix.example.org/fhir/",
        ] {
            let url = endpoint(Url::parse(base).expect("a URL")).expect("an endpoint");
            assert_eq!(
                url.as_str(),
                "https://pix.example.org/fhir/Patient/$ihe-pix",
                "{base}"
            );
        }
    }

    #[test]
    fn a_base_with_a_query_or_another_scheme_is_refused() {
        for base in [
            "https://pix.example.org/fhir?x=1",
            "https://pix.example.org/fhir#top",
            "ftp://pix.example.org/fhir",
            "urn:oid:2.999.1",
        ] {
            assert_eq!(
                endpoint(Url::parse(base).expect("a URL")).err(),
                Some(InvalidInput::Base),
                "{base} is no FHIR base"
            );
        }
    }

    #[test]
    fn the_separators_fhir_search_defines_are_escaped() {
        assert_eq!(escape(r"a|b,c$d\e"), r"a\|b\,c\$d\\e", "every separator");
        assert_eq!(
            escape("IHERED-994"),
            "IHERED-994",
            "plain text is unchanged"
        );
    }

    #[test]
    fn the_query_names_the_source_once_and_each_target() {
        let source = SourceIdentifier::new("urn:oid:2.999.1", SecretString::from("x|1"))
            .expect("a source identifier");
        let targets = [
            TargetSystem::new("urn:oid:2.999.2").expect("a target"),
            TargetSystem::new("urn:oid:2.999.3").expect("a target"),
        ];
        let base = endpoint(Url::parse("https://pix.example.org/fhir/").expect("a URL"))
            .expect("an endpoint");
        let url = query(&base, &source, &targets);
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert_eq!(
            pairs,
            [
                (
                    "sourceIdentifier".to_owned(),
                    r"urn:oid:2.999.1|x\|1".to_owned()
                ),
                ("targetSystem".to_owned(), "urn:oid:2.999.2".to_owned()),
                ("targetSystem".to_owned(), "urn:oid:2.999.3".to_owned()),
            ],
            "the value's own | is escaped, the separator is not"
        );
    }
}
