// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[server]` section: the listener, the base path, the public base URL,
//! the body ceiling, the request timeout and the drain window.
//!
//! No specification governs the listener: our own design. The public base URL
//! is the `[base]` FHIR R4 names every resource location under
//! (<https://hl7.org/fhir/R4/http.html#general>), and the identifier system
//! of the PMIR Master Patient Identification Domain (PMIR 1.6.0
//! §2:3.93.4.1.3).

use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Profile;
use crate::config::error::Error;
use crate::config::section::{is_loopback_url, socket_address, url};

/// The `[server]` section as written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Server {
    /// The socket address the HTTP listener binds.
    pub listen: String,
    /// The path every route is mounted under: empty for the root, or a path
    /// that starts with `/` and does not end with one.
    pub base_path: String,
    /// The URL a client reaches `{base}` at.
    pub public_base_url: String,
    /// The largest request body read before the server answers `413`.
    pub body_limit_bytes: usize,
    /// How long one request may take before the server answers `408`.
    pub request_timeout_ms: u64,
    /// How long the requests in flight may take to finish after `SIGTERM`.
    pub drain_timeout_ms: u64,
}

impl Default for Server {
    fn default() -> Self {
        Self {
            listen: String::from("127.0.0.1:8080"),
            base_path: String::new(),
            public_base_url: String::new(),
            body_limit_bytes: 1024 * 1024,
            request_timeout_ms: 30_000,
            drain_timeout_ms: 10_000,
        }
    }
}

/// The `[server]` section, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSettings {
    /// The socket address the HTTP listener binds.
    pub listen: SocketAddr,
    /// The path every route is mounted under, empty for the root.
    pub base_path: String,
    /// The URL a client reaches `{base}` at, without a trailing `/`.
    pub public_base_url: url::Url,
    /// The largest request body read before the server answers `413`.
    pub body_limit: usize,
    /// How long one request may take before the server answers `408`.
    pub request_timeout: Duration,
    /// How long the requests in flight may take to finish after `SIGTERM`.
    pub drain_timeout: Duration,
}

impl Server {
    /// Resolves the section under `profile`.
    ///
    /// # Errors
    /// Returns [`Error::Listen`] for a listener that does not parse,
    /// [`Error::Missing`] for an unset public base URL, [`Error::Url`] for
    /// one that does not parse, [`Error::Value`] for a base path, a public
    /// base URL or a bound outside its rule, and [`Error::DevelopmentOnly`]
    /// for a plain `http` public base URL on a host other than a loopback one
    /// under production.
    pub fn resolve(&self, profile: Profile) -> Result<ServerSettings, Error> {
        let listen = socket_address("server.listen", &self.listen)?;
        if !base_path_is_valid(&self.base_path) {
            return Err(Error::Value {
                key: String::from("server.base_path"),
                rule: "empty, or a path of segments that starts with / and does not end with one",
            });
        }
        let public_base_url = self.public_base_url(profile)?;
        if self.body_limit_bytes == 0 {
            return Err(value("server.body_limit_bytes", "it must be above 0"));
        }
        if self.request_timeout_ms == 0 {
            return Err(value("server.request_timeout_ms", "it must be above 0"));
        }
        Ok(ServerSettings {
            listen,
            base_path: self.base_path.clone(),
            public_base_url,
            body_limit: self.body_limit_bytes,
            request_timeout: Duration::from_millis(self.request_timeout_ms),
            drain_timeout: Duration::from_millis(self.drain_timeout_ms),
        })
    }

    /// Resolves `public_base_url`.
    fn public_base_url(&self, profile: Profile) -> Result<url::Url, Error> {
        const KEY: &str = "server.public_base_url";
        if self.public_base_url.is_empty() {
            return Err(Error::Missing {
                key: KEY.to_owned(),
            });
        }
        let parsed = url(KEY, self.public_base_url.trim_end_matches('/'))?;
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
        if parsed.scheme() == "http" && profile == Profile::Production && !is_loopback_url(&parsed)
        {
            return Err(Error::DevelopmentOnly {
                key: KEY.to_owned(),
                rule: "a plain http base on a host other than a loopback one",
            });
        }
        Ok(parsed)
    }
}

/// Returns an [`Error::Value`] under `key`.
fn value(key: &str, rule: &'static str) -> Error {
    Error::Value {
        key: key.to_owned(),
        rule,
    }
}

/// Returns whether `path` is empty or `/segment(/segment)*`, each segment of
/// the RFC 3986 §3.3 unreserved characters.
fn base_path_is_valid(path: &str) -> bool {
    if path.is_empty() {
        return true;
    }
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    rest.split('/').all(|segment| {
        !segment.is_empty()
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~'))
    })
}

#[cfg(test)]
mod tests {
    use super::{Server, base_path_is_valid};
    use crate::config::Profile;
    use crate::config::error::Error;

    fn server(public_base_url: &str) -> Server {
        Server {
            public_base_url: public_base_url.to_owned(),
            ..Server::default()
        }
    }

    #[test]
    fn a_base_path_is_empty_or_slash_separated_segments() {
        for good in ["", "/pix", "/a/b-c/d_e.f~g"] {
            assert!(base_path_is_valid(good), "{good:?}");
        }
        for bad in [
            "/", "pix", "/pix/", "//pix", "/p ix", "/pix?x", "/pix#x", "/%41",
        ] {
            assert!(!base_path_is_valid(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_public_base_url_is_required_and_its_trailing_slash_dropped() {
        let error = server("")
            .resolve(Profile::Production)
            .expect_err("the public base URL has no default");
        assert!(matches!(error, Error::Missing { .. }), "{error:?}");
        let resolved = server("https://pix.example.org/base/")
            .resolve(Profile::Production)
            .expect("an https base resolves");
        assert_eq!(
            "https://pix.example.org/base",
            resolved.public_base_url.as_str()
        );
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
            let error = server(bad)
                .resolve(Profile::Development)
                .expect_err("refused");
            assert!(matches!(error, Error::Value { .. }), "{bad}: {error:?}");
        }
    }

    #[test]
    fn a_plain_http_base_is_refused_under_production_unless_it_is_loopback() {
        let error = server("http://pix.example.org")
            .resolve(Profile::Production)
            .expect_err("production names a TLS base");
        assert!(matches!(error, Error::DevelopmentOnly { .. }), "{error:?}");
        server("http://127.0.0.1:8080")
            .resolve(Profile::Production)
            .expect("a loopback base may be plain http");
        server("http://pix.example.org")
            .resolve(Profile::Development)
            .expect("development admits a plain http base");
    }

    #[test]
    fn a_listener_that_does_not_parse_and_a_zero_bound_are_refused() {
        let mut bad = server("https://pix.example.org");
        bad.listen = String::from("localhost");
        assert!(matches!(
            bad.resolve(Profile::Production),
            Err(Error::Listen { .. })
        ));
        let mut zero = server("https://pix.example.org");
        zero.body_limit_bytes = 0;
        assert!(matches!(
            zero.resolve(Profile::Production),
            Err(Error::Value { .. })
        ));
        let mut zero = server("https://pix.example.org");
        zero.request_timeout_ms = 0;
        assert!(matches!(
            zero.resolve(Profile::Production),
            Err(Error::Value { .. })
        ));
    }
}
