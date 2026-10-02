// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-78 request: `POST [base]/Patient/_search` with the criteria in an
//! `application/x-www-form-urlencoded` body (§2:3.78.4.1.2).

use url::Url;

use crate::search;

use super::error::InvalidInput;

/// The search on the Supplier's `Patient` type, relative to the FHIR base
/// (FHIR R4 search, <http://hl7.org/fhir/R4/http.html#search>).
const SEARCH: &str = "Patient/_search";

/// The media type of the search body.
pub(super) const FORM: &str = "application/x-www-form-urlencoded";

/// The FHIR base `base`, with the trailing `/` a relative link resolves under.
pub(super) fn base(base: Url) -> Result<Url, InvalidInput> {
    search::under_base(base, "").ok_or(InvalidInput::Base)
}

/// The `[base]/Patient/_search` URL for the FHIR base `base`.
pub(super) fn endpoint(base: Url) -> Result<Url, InvalidInput> {
    search::under_base(base, SEARCH).ok_or(InvalidInput::Base)
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{base, endpoint};
    use crate::pdqm::error::InvalidInput;

    #[test]
    fn the_endpoint_is_the_type_level_search_under_the_base() {
        for given in [
            "https://pdq.example.org/fhir",
            "https://pdq.example.org/fhir/",
        ] {
            let url = endpoint(Url::parse(given).expect("a URL")).expect("an endpoint");
            assert_eq!(
                url.as_str(),
                "https://pdq.example.org/fhir/Patient/_search",
                "{given}"
            );
            let url = base(Url::parse(given).expect("a URL")).expect("a base");
            assert_eq!(url.as_str(), "https://pdq.example.org/fhir/", "{given}");
        }
    }

    #[test]
    fn a_base_with_a_query_or_another_scheme_is_refused() {
        for base in [
            "https://pdq.example.org/fhir?x=1",
            "https://pdq.example.org/fhir#top",
            "ftp://pdq.example.org/fhir",
            "urn:oid:2.999.1",
        ] {
            assert_eq!(
                endpoint(Url::parse(base).expect("a URL")).err(),
                Some(InvalidInput::Base),
                "{base} is no FHIR base"
            );
        }
    }
}
