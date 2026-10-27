// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The public base URL: the root key `public_base_url`, the URL a client
//! reaches `{base}` at.
//!
//! It is the `[base]` FHIR R4 names every resource location under
//! (<https://hl7.org/fhir/R4/http.html#general>), and the identifier system
//! of the PMIR Master Patient Identification Domain (PMIR 1.6.0
//! §2:3.93.4.1.3). `[server]` itself is `ferrohealth-server`'s
//! `ServerSection`.

use crate::config::Profile;
use crate::config::error::Error;
use crate::config::section::{is_loopback_url, url};

/// The key the public base URL is set under.
pub const KEY: &str = "public_base_url";

/// Resolves the public base URL `text` under `profile`, without its trailing
/// `/`.
///
/// # Errors
/// Returns [`Error::Missing`] for an unset URL, [`Error::Url`] for one that
/// does not parse, [`Error::Value`] for one outside its rule, and
/// [`Error::DevelopmentOnly`] for a plain `http` URL on a host other than a
/// loopback one under production.
pub fn resolve(text: &str, profile: Profile) -> Result<url::Url, Error> {
    if text.is_empty() {
        return Err(Error::Missing {
            key: KEY.to_owned(),
        });
    }
    let parsed = url(KEY, text.trim_end_matches('/'))?;
    if !matches!(parsed.scheme(), "https" | "http")
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(value(
            KEY,
            "an http or https URL with a host and no userinfo, query or fragment",
        ));
    }
    // NOTE: no specification governs this: our own design; a production
    // server names a TLS base, and only a loopback base may be plain http.
    if parsed.scheme() == "http" && profile == Profile::Production && !is_loopback_url(&parsed) {
        return Err(Error::DevelopmentOnly {
            key: KEY.to_owned(),
            rule: "a plain http base on a host other than a loopback one",
        });
    }
    Ok(parsed)
}

/// Returns an [`Error::Value`] under `key`.
fn value(key: &str, rule: &'static str) -> Error {
    Error::Value {
        key: key.to_owned(),
        rule,
    }
}

#[cfg(test)]
mod tests {
    use super::resolve;
    use crate::config::Profile;
    use crate::config::error::Error;

    #[test]
    fn the_public_base_url_is_required_and_its_trailing_slash_dropped() {
        let error =
            resolve("", Profile::Production).expect_err("the public base URL has no default");
        assert!(matches!(error, Error::Missing { .. }), "{error:?}");
        let resolved = resolve("https://pix.example.org/base/", Profile::Production)
            .expect("an https base resolves");
        assert_eq!("https://pix.example.org/base", resolved.as_str());
    }

    #[test]
    fn a_public_base_url_with_userinfo_a_query_a_fragment_or_another_scheme_is_refused() {
        for bad in [
            "https://u:p@pix.example.org",
            "https://pix.example.org?x=1",
            "https://pix.example.org#x",
            "ftp://pix.example.org",
            "urn:oid:1.2.3",
        ] {
            let error = resolve(bad, Profile::Development).expect_err("refused");
            assert!(matches!(error, Error::Value { .. }), "{bad}: {error:?}");
        }
    }

    #[test]
    fn a_plain_http_base_is_refused_under_production_unless_it_is_loopback() {
        let error = resolve("http://pix.example.org", Profile::Production)
            .expect_err("production names a TLS base");
        assert!(matches!(error, Error::DevelopmentOnly { .. }), "{error:?}");
        resolve("http://127.0.0.1:8080", Profile::Production)
            .expect("a loopback base may be plain http");
        resolve("http://pix.example.org", Profile::Development)
            .expect("development admits a plain http base");
    }
}
