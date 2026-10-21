// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The value syntax of a FHIR search parameter
//! (FHIR R4 search, <http://hl7.org/fhir/R4/search.html>). The URL of an
//! interaction under a FHIR base is `ihe_atna::fhir_base`'s.

/// Returns `part` with the characters FHIR search gives a meaning to escaped
/// by a backslash: `\`, `|`, `,` and `$` (FHIR R4 search, Escaping Search
/// Parameters, <http://hl7.org/fhir/R4/search.html#escaping>).
#[cfg(any(feature = "pixm", feature = "pdqm", feature = "mcsd", feature = "pmir"))]
pub(crate) fn escape(part: &str) -> String {
    let mut escaped = String::with_capacity(part.len());
    for character in part.chars() {
        if matches!(character, '\\' | '|' | ',' | '$') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(all(
    test,
    any(feature = "pixm", feature = "pdqm", feature = "mcsd", feature = "pmir")
))]
mod tests {
    use super::escape;

    #[test]
    fn the_separators_fhir_search_defines_are_escaped() {
        assert_eq!(escape(r"a|b,c$d\e"), r"a\|b\,c\$d\\e", "every separator");
        assert_eq!(
            escape("IHERED-994"),
            "IHERED-994",
            "plain text is unchanged"
        );
    }
}
