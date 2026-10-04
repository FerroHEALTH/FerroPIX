// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `Debug` rendering of a URL that may carry a credential.
//!
//! A URL may carry a user name and password in its userinfo (RFC 3986
//! §3.2.1) and a credential in its query, so every type of this crate that
//! holds one shows it through [`RedactedUrl`] with both replaced by [`REDACTED`]. No
//! specification governs this: our own design.

#[cfg(any(
    feature = "pixm",
    feature = "pdqm",
    feature = "mcsd",
    feature = "pmir",
    feature = "xcpd"
))]
use std::fmt;

/// The fixed text a rendering shows in place of a credential.
pub(crate) const REDACTED: &str = "***";

/// A URL whose `Debug` shows it with its userinfo and its query redacted.
#[cfg(any(
    feature = "pixm",
    feature = "pdqm",
    feature = "mcsd",
    feature = "pmir",
    feature = "xcpd"
))]
pub(crate) struct RedactedUrl<'a>(pub(crate) &'a str);

#[cfg(any(
    feature = "pixm",
    feature = "pdqm",
    feature = "mcsd",
    feature = "pmir",
    feature = "xcpd"
))]
impl fmt::Debug for RedactedUrl<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&redact(self.0), f)
    }
}

/// Returns `url` with its userinfo and its query replaced by [`REDACTED`].
///
/// The userinfo is everything after `://` up to an `@`, since a host holds no
/// `@` (RFC 3986 §3.2). When the text parses as a URL (the WHATWG URL
/// Standard, as `url` and every client built on it read it), the authority
/// ends at its first `/`, `?` or `#`, so an `@` after it is in the path
/// or the query and shows as written. When it does not parse, the userinfo
/// runs to its last `@`, so a password holding a `/`, a `?` or a `#` is still
/// found. The userinfo becomes `***@` and the query `?***`; the scheme, the
/// host, the port, the path and the fragment show as written. Text with no
/// `://` has no authority to find a credential in, so it shows as
/// [`REDACTED`] whole. The text is never decoded, so it is redacted as
/// written.
#[cfg(any(
    feature = "pixm",
    feature = "pdqm",
    feature = "mcsd",
    feature = "pmir",
    feature = "xcpd"
))]
fn redact(url: &str) -> String {
    if url.is_empty() {
        return String::new();
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return REDACTED.to_owned();
    };
    let at = if url::Url::parse(url).is_ok() {
        let authority = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        rest.get(..authority).and_then(|text| text.rfind('@'))
    } else {
        rest.rfind('@')
    };
    let (userinfo, rest) = match at {
        Some(at) => (
            format!("{REDACTED}@"),
            rest.get(at + 1..).unwrap_or_default(),
        ),
        None => (String::new(), rest),
    };
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    let (hierarchy, tail) = rest.split_at_checked(end).unwrap_or((rest, ""));
    let tail = match tail.strip_prefix('?') {
        Some(query) => {
            let fragment = query
                .find('#')
                .and_then(|at| query.get(at..))
                .unwrap_or_default();
            format!("?{REDACTED}{fragment}")
        }
        None => tail.to_owned(),
    };
    format!("{scheme}://{userinfo}{hierarchy}{tail}")
}

#[cfg(all(
    test,
    any(
        feature = "pixm",
        feature = "pdqm",
        feature = "mcsd",
        feature = "pmir",
        feature = "xcpd"
    )
))]
mod tests {
    use super::{REDACTED, RedactedUrl, redact};

    #[test]
    fn every_part_that_may_carry_a_credential_is_redacted() {
        for (raw, redacted) in [
            (
                "https://Qz7user@pix.example.org/fhir",
                "https://***@pix.example.org/fhir",
            ),
            (
                "https://Qz7user:Qz7password@pix.example.org:8443/fhir/Patient/$ihe-pix",
                "https://***@pix.example.org:8443/fhir/Patient/$ihe-pix",
            ),
            (
                "https://u:p@ss@pix.example.org/",
                "https://***@pix.example.org/",
            ),
            (
                "https://u:p@pix.example.org?x=1",
                "https://***@pix.example.org?***",
            ),
            (
                "https://pdq.example.org/fhir/Patient?_page=Qz7token#f",
                "https://pdq.example.org/fhir/Patient?***#f",
            ),
            ("urn:uuid:1e7f2a52-0d1c-4b0f-9a52-5d3c1c0e2f11", REDACTED),
            ("u:Qz7password@pix.example.org/x", REDACTED),
        ] {
            assert_eq!(redacted, redact(raw), "{raw}");
        }
    }

    #[test]
    fn a_password_holding_a_delimiter_is_redacted() {
        for (raw, redacted) in [
            ("https://u:p/x@host", "https://***@host"),
            (
                "https://u:Qz7/pass@pix.example.org/fhir",
                "https://***@pix.example.org/fhir",
            ),
            (
                "https://u:Qz7?pass@pix.example.org/fhir?x=1",
                "https://***@pix.example.org/fhir?***",
            ),
            (
                "https://u:Qz7#pass@pix.example.org/fhir",
                "https://***@pix.example.org/fhir",
            ),
        ] {
            assert_eq!(redacted, redact(raw), "{raw}");
        }
    }

    #[test]
    fn an_at_sign_after_the_authority_is_not_userinfo() {
        for (raw, redacted) in [
            (
                "https://directory.example.org/fhir/Endpoint/@handle",
                "https://directory.example.org/fhir/Endpoint/@handle",
            ),
            (
                "https://pix.example.org/fhir/a@b/Patient",
                "https://pix.example.org/fhir/a@b/Patient",
            ),
            (
                "https://pix.example.org/fhir/Patient?email=someone@example.org",
                "https://pix.example.org/fhir/Patient?***",
            ),
            (
                "https://pix.example.org/fhir#section@b",
                "https://pix.example.org/fhir#section@b",
            ),
        ] {
            assert_eq!(redacted, redact(raw), "{raw}");
        }
    }

    #[test]
    fn text_that_does_not_parse_is_redacted_to_its_last_at_sign() {
        assert_eq!(
            "https://***@handle",
            redact("https://pix example.org/fhir/@handle")
        );
    }

    #[test]
    fn a_url_without_a_credential_shows_as_written() {
        for raw in [
            "https://pix.example.org/fhir/Patient/$ihe-pix",
            "https://directory.example.org/fhir/Endpoint/@handle",
            "http://127.0.0.1:8080/fhir/",
            "",
        ] {
            assert_eq!(raw, redact(raw), "{raw}");
        }
    }

    #[test]
    fn debug_quotes_the_redacted_url() {
        let shown = format!(
            "{:?}",
            RedactedUrl("https://u:Qz7password@pix.example.org/")
        );
        assert_eq!("\"https://***@pix.example.org/\"", shown);
    }
}
