// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FHIR R4 REST pieces every FHIR client shares, with no FHIR model.
//!
//! They are the value syntax of a search parameter
//! (<http://hl7.org/fhir/R4/search.html>) and the JSON media types of an
//! answer (<http://hl7.org/fhir/R4/http.html#mime-type>). The URL of an
//! interaction under a FHIR base is `ihe_atna::fhir_base`'s.

/// Returns `part` with the characters FHIR search gives a meaning to escaped
/// by a backslash: `\`, `|`, `,` and `$` (FHIR R4 search, Escaping Search
/// Parameters, <http://hl7.org/fhir/R4/search.html#escaping>).
#[must_use]
pub fn escape(part: &str) -> String {
    let mut escaped = String::with_capacity(part.len());
    for character in part.chars() {
        if matches!(character, '\\' | '|' | ',' | '$') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// Whether `media` is FHIR JSON (ITI TF-2 Appendix Z.6), or plain JSON, which
/// FHIR R4 accepts for the same content (<http://hl7.org/fhir/R4/http.html#mime-type>).
#[must_use]
pub fn fhir_json(media: Option<&str>) -> bool {
    media
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|essence| {
            essence.eq_ignore_ascii_case("application/fhir+json")
                || essence.eq_ignore_ascii_case("application/json")
        })
}

#[cfg(test)]
mod tests {
    use super::{escape, fhir_json};

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
    fn fhir_json_and_plain_json_are_accepted_with_parameters() {
        assert!(fhir_json(Some("application/fhir+json; fhirVersion=4.0")));
        assert!(fhir_json(Some("Application/JSON")));
        assert!(!fhir_json(Some("text/html")));
        assert!(!fhir_json(None));
    }
}
