// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What a server reads a search or an operation parameter with: the pairs of
//! a URL query or a form, and a token in the shape the PIXm and PDQm readers
//! take, read by the family's FHIR search grammar (`fhir-search`, FHIR R4
//! search.html §3.1.1.4).

use fhir_search::token::{Token as Grammar, TokenError};
use secrecy::SecretString;

/// The name and value pairs of a URL query (without its `?`), each
/// percent-decoded as RFC 3986 §2.1 reads a URL, so a `+` stands for itself.
pub(crate) fn url_query(query: &str) -> Vec<(String, String)> {
    form(query.replace('+', "%2B").as_bytes())
}

/// The name and value pairs of an `application/x-www-form-urlencoded` body,
/// in which a `+` stands for a space (WHATWG URL, the
/// `application/x-www-form-urlencoded` parser).
pub(crate) fn form(body: &[u8]) -> Vec<(String, String)> {
    url::form_urlencoded::parse(body).into_owned().collect()
}

/// Whether `text` holds only what a FHIR `string` may: the lexical form
/// `[ \r\n\t\S]+` of FHIR R4 datatypes.html#primitive, read as an XML Schema
/// pattern, so of the whitespace only the space, the tab, the carriage
/// return and the line feed. Emptiness is the caller's to judge.
#[cfg(feature = "pixm-server")]
pub(crate) fn fhir_string(text: &str) -> bool {
    !text.contains(['\u{b}', '\u{c}'])
}

/// A token value: `[code]`, `[system]|[code]`, `|[code]` or `[system]|`
/// (FHIR R4 search, token, <http://hl7.org/fhir/R4/search.html#token>).
///
/// The code is a secret: it is an identifier or a contact value.
pub(crate) struct Token {
    /// The system: `None` when the token has no `|`, empty for `|[code]`.
    pub(crate) system: Option<String>,
    /// The code, empty for `[system]|`.
    pub(crate) code: SecretString,
}

/// The token `text` holds, as the escaped part a split gave.
///
/// # Errors
/// A [`TokenError`] for an illegal escape, a second unescaped `|`, or a
/// value that names neither a system nor a code.
pub(crate) fn token(text: &str) -> Result<Token, TokenError> {
    let (system, code) = match Grammar::parse(text)? {
        Grammar::Code(code) => (None, code),
        Grammar::NoSystem(code) => (Some(String::new()), code),
        Grammar::SystemCode { system, code } => (Some(system), code),
        Grammar::System(system) => (Some(system), String::new()),
    };
    Ok(Token {
        system,
        code: SecretString::from(code),
    })
}

#[cfg(test)]
mod tests {
    use fhir_search::escape::{EscapeError, split, unescape};
    use fhir_search::token::TokenError;
    use secrecy::ExposeSecret;

    use super::token;

    #[test]
    fn an_escaped_separator_does_not_split() {
        assert_eq!(split(r"a\,b,c", ','), [r"a\,b", "c"], "the escape is kept");
        assert_eq!(split("a,", ','), ["a", ""], "a trailing separator");
        assert_eq!(split(r"a\\,b", ','), [r"a\\", "b"], "an escaped backslash");
        assert_eq!(
            unescape(r"x\|1\,2\$3\\4").as_deref(),
            Ok(r"x|1,2$3\4"),
            "the four escapes"
        );
        assert_eq!(
            unescape(r"x\n"),
            Err(EscapeError::Illegal { at: 1 }),
            "any other escape is illegal (FHIR R4 search.html §3.1.1.4.19)"
        );
    }

    #[test]
    #[cfg(feature = "pixm-server")]
    fn a_fhir_string_holds_no_vertical_tab_or_form_feed() {
        assert!(
            super::fhir_string("van Dam\r\n\t\u{85}"),
            "the four and non-ASCII"
        );
        assert!(!super::fhir_string("IH\u{b}RED"), "a vertical tab");
        assert!(!super::fhir_string("IH\u{c}RED"), "a form feed");
    }

    #[test]
    fn a_plus_is_itself_in_a_url_and_a_space_in_a_form() {
        let pairs = |list: Vec<(String, String)>| {
            list.into_iter().map(|(_, value)| value).collect::<Vec<_>>()
        };
        assert_eq!(
            pairs(super::url_query("_format=application/fhir+xml")),
            ["application/fhir+xml"]
        );
        assert_eq!(pairs(super::form(b"family=van+Dam")), ["van Dam"]);
    }

    #[test]
    fn a_token_is_split_at_its_first_unescaped_bar() {
        let full = token(r"urn:oid:2.999.1|a\|1").expect("a token");
        assert_eq!(full.system.as_deref(), Some("urn:oid:2.999.1"));
        assert_eq!(
            full.code.expose_secret(),
            "a|1",
            "the escaped bar is the value's"
        );
        let bare = token("8675309").expect("a token");
        assert_eq!(bare.system, None, "no system part");
        let none = token("|8675309").expect("a token");
        assert_eq!(none.system.as_deref(), Some(""), "the empty system");
        let domain = token("urn:oid:2.999.1|").expect("a token");
        assert_eq!(domain.code.expose_secret(), "", "any code in the system");
    }

    #[test]
    fn a_token_outside_the_four_forms_is_refused() {
        assert_eq!(token("|").err(), Some(TokenError::Empty));
        assert_eq!(token("a|b|c").err(), Some(TokenError::Bars));
        assert!(matches!(token(r"a\x"), Err(TokenError::Escape(_))));
    }
}
