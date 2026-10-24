// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The value syntax a server reads from a search or an operation parameter:
//! the `,` between alternatives, the `|` of a token, and the backslash that
//! escapes either (FHIR R4 search, "Escaping Search Parameters",
//! <http://hl7.org/fhir/R4/search.html#escaping>).

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

/// The parts of `text` between each `separator` no backslash escapes, every
/// escape kept for [`unescape`].
pub(crate) fn split(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == separator {
            parts.push(text.get(start..index).unwrap_or_default());
            start = index.saturating_add(character.len_utf8());
        }
    }
    parts.push(text.get(start..).unwrap_or_default());
    parts
}

/// `text` with each escaped `\`, `|`, `,` and `$` read as itself; a backslash
/// before any other character is kept as written.
pub(crate) fn unescape(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\'
            && let Some(&next) = characters.peek()
            && matches!(next, '\\' | '|' | ',' | '$')
        {
            plain.push(next);
            characters.next();
        } else {
            plain.push(character);
        }
    }
    plain
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

/// The token `text` holds, its parts unescaped.
pub(crate) fn token(text: &str) -> Token {
    match split(text, '|').as_slice() {
        [code] => Token {
            system: None,
            code: SecretString::from(unescape(code)),
        },
        [system, rest @ ..] => Token {
            system: Some(unescape(system)),
            code: SecretString::from(unescape(&rest.join("|"))),
        },
        [] => Token {
            system: None,
            code: SecretString::from(""),
        },
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::{split, token, unescape};

    #[test]
    fn an_escaped_separator_does_not_split() {
        assert_eq!(split(r"a\,b,c", ','), [r"a\,b", "c"], "the escape is kept");
        assert_eq!(split("a,", ','), ["a", ""], "a trailing separator");
        assert_eq!(split(r"a\\,b", ','), [r"a\\", "b"], "an escaped backslash");
        assert_eq!(
            unescape(r"x\|1\,2\$3\\4\n"),
            r"x|1,2$3\4\n",
            "the four escapes"
        );
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
        let full = token(r"urn:oid:2.999.1|a\|1");
        assert_eq!(full.system.as_deref(), Some("urn:oid:2.999.1"));
        assert_eq!(
            full.code.expose_secret(),
            "a|1",
            "the escaped bar is the value's"
        );
        let bare = token("8675309");
        assert_eq!(bare.system, None, "no system part");
        let none = token("|8675309");
        assert_eq!(none.system.as_deref(), Some(""), "the empty system");
        let domain = token("urn:oid:2.999.1|");
        assert_eq!(domain.code.expose_secret(), "", "any code in the system");
    }
}
