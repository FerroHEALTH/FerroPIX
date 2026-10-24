// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The two types every secret of the configuration is held in, and the rule
//! that reads one from its key or from its `<key>_file` sibling.
//!
//! Redaction is a property of the type. A [`Secret`] renders as [`REDACTED`]
//! through `Debug`, `Display` and `Serialize`; a [`SecretUrl`] (a URL that may
//! carry a user name and password in its userinfo, RFC 3986 §3.2.1, or a
//! credential in its query) renders with those parts replaced by
//! [`REDACTED`]. Each holds its value in a [`SecretString`], zeroed on drop,
//! and is read only through `expose`. No specification governs this: our own
//! design, after FerroFED's `ferrofed_registry::secret`.

use std::fmt;
use std::path::Path;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::Profile;
use crate::config::error::Error;

/// The fixed text every rendering of a secret shows in its place.
pub const REDACTED: &str = "***";

/// A secret string that never renders itself.
///
/// # Examples
///
/// ```
/// use ferropix_server::config::secret::Secret;
///
/// let key = Secret::new("Qz7-not-a-real-key");
/// assert_eq!(format!("{key:?}"), "\"***\"");
/// assert_eq!(key.to_string(), "***");
/// assert_eq!(key.expose(), "Qz7-not-a-real-key");
/// ```
#[derive(Clone)]
pub struct Secret(SecretString);

impl Secret {
    /// Creates a secret holding `value`.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(SecretString::from(value.into()))
    }

    /// Returns the value, for the one place it is used.
    ///
    /// Never log or format the result.
    #[must_use]
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    /// Returns whether the secret is the empty string.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.expose_secret().is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(REDACTED, f)
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::new)
    }
}

impl Serialize for Secret {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(REDACTED)
    }
}

/// A URL or connection string kept as written, whose renderings never show a
/// credential.
///
/// # Examples
///
/// ```
/// use ferropix_server::config::secret::SecretUrl;
///
/// let url = SecretUrl::new("postgres://ferropix:Qz7pw@db.example.org:5432/pix?sslmode=require");
/// assert_eq!(url.to_string(), "postgres://***@db.example.org:5432/pix?***");
/// assert!(url.expose().contains("Qz7pw"));
/// ```
#[derive(Clone)]
pub struct SecretUrl(SecretString);

impl SecretUrl {
    /// Creates a URL holding `url` verbatim.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self(SecretString::from(url.into()))
    }

    /// Returns the URL as written, credentials included, for connecting with
    /// it.
    ///
    /// Never log or format the result.
    #[must_use]
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    /// Returns the URL with every part that may carry a credential replaced
    /// by [`REDACTED`].
    ///
    /// The userinfo becomes `***@` and the query becomes `?***`; the scheme,
    /// the host, the port, the path and the fragment show as written. Text
    /// with no `://` shows as [`REDACTED`] whole, since it may be a key and
    /// value connection string with its password outside any userinfo.
    #[must_use]
    pub fn redacted(&self) -> String {
        redact(self.0.expose_secret())
    }
}

/// Returns `url` with its userinfo and its query replaced by [`REDACTED`].
///
/// The authority runs from `://` to the first `/`, `?` or `#` after the last
/// `@` that precedes it, so a password holding one of those characters is
/// still inside the replaced userinfo (RFC 3986 §3.2: a host holds no `@`).
fn redact(url: &str) -> String {
    if url.is_empty() {
        return String::new();
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return REDACTED.to_owned();
    };
    let (userinfo, rest) = match rest.rfind('@') {
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

impl fmt::Debug for SecretUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.redacted(), f)
    }
}

impl fmt::Display for SecretUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

impl<'de> Deserialize<'de> for SecretUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::new)
    }
}

impl Serialize for SecretUrl {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.redacted())
    }
}

/// Returns the text of the secret set under `key` or in the file its
/// `<key>_file` sibling names, or `None` when neither is set.
///
/// The file's text is trimmed and the untrimmed read is zeroed on drop. Under
/// [`Profile::Production`] a secret set under `key` itself is refused, so a
/// value never sits in the configuration file or the environment of a
/// production server (no specification governs this: our own design).
///
/// # Errors
/// Returns [`Error::SecretConflict`] when both are set,
/// [`Error::InlineSecret`] for an inline value under production,
/// [`Error::SecretFile`] when the file cannot be read and
/// [`Error::EmptySecret`] when it holds only whitespace.
pub fn read(
    key: &str,
    inline: Option<&Secret>,
    file: Option<&Path>,
    profile: Profile,
) -> Result<Option<Secret>, Error> {
    match (inline, file) {
        (Some(_), Some(_)) => Err(Error::SecretConflict {
            key: key.to_owned(),
        }),
        (Some(_), None) if profile == Profile::Production => Err(Error::InlineSecret {
            key: key.to_owned(),
        }),
        (Some(value), None) => Ok(Some(value.clone())),
        (None, Some(path)) => read_file(&format!("{key}_file"), path).map(Some),
        (None, None) => Ok(None),
    }
}

/// Reads the secret file `path` under `key`, trimmed and refused when empty.
///
/// # Errors
/// Returns [`Error::SecretFile`] when the file cannot be read and
/// [`Error::EmptySecret`] when it holds only whitespace.
pub fn read_file(key: &str, path: &Path) -> Result<Secret, Error> {
    let text = std::fs::read_to_string(path)
        .map(SecretString::from)
        .map_err(|source| Error::SecretFile {
            key: key.to_owned(),
            path: path.to_path_buf(),
            source,
        })?;
    let value = text.expose_secret().trim();
    if value.is_empty() {
        return Err(Error::EmptySecret {
            key: key.to_owned(),
            path: path.to_path_buf(),
        });
    }
    Ok(Secret::new(value))
}

#[cfg(test)]
mod tests {
    use super::{REDACTED, Secret, SecretUrl, read};
    use crate::config::Profile;
    use crate::config::error::Error;

    #[test]
    fn a_secret_renders_as_three_stars_through_every_rendering() {
        let secret = Secret::new("Qz7-not-a-real-key");
        assert_eq!("\"***\"", format!("{secret:?}"));
        assert_eq!(REDACTED, secret.to_string());
        assert_eq!(REDACTED, format!("{secret:#?}").trim_matches('"'));
        let serialized = serde_json::to_string(&secret).expect("a secret serializes");
        assert_eq!("\"***\"", serialized);
        assert!(!serialized.contains("Qz7"), "the value must not be written");
    }

    #[test]
    fn a_secret_url_hides_its_userinfo_and_its_query_and_keeps_the_rest() {
        for (written, shown) in [
            (
                "postgres://ferropix:Qz7pw@db.example.org:5432/pix",
                "postgres://***@db.example.org:5432/pix",
            ),
            (
                "postgresql://db.example.org/pix?password=Qz7pw&sslmode=require",
                "postgresql://db.example.org/pix?***",
            ),
            (
                "postgres://u:p%40ss/w?rd@db.example.org/pix?sslmode=verify-full#tag",
                "postgres://***@db.example.org/pix?***#tag",
            ),
            (
                "postgres://db.example.org:5432/pix",
                "postgres://db.example.org:5432/pix",
            ),
            ("host=db password=Qz7pw", REDACTED),
            ("", ""),
        ] {
            let url = SecretUrl::new(written);
            assert_eq!(shown, url.to_string(), "{written}");
            assert_eq!(format!("{shown:?}"), format!("{url:?}"), "{written}");
            assert_eq!(
                format!("\"{shown}\""),
                serde_json::to_string(&url).expect("a URL serializes"),
                "{written}"
            );
            assert_eq!(written, url.expose());
        }
    }

    #[test]
    fn an_inline_secret_is_refused_under_production_and_kept_under_development() {
        let inline = Secret::new("postgres://pix.invalid/pix");
        let error = read("db.url", Some(&inline), None, Profile::Production)
            .expect_err("production takes a secret from its file only");
        assert!(
            matches!(&error, Error::InlineSecret { key } if key == "db.url"),
            "{error:?}"
        );
        let kept = read("db.url", Some(&inline), None, Profile::Development)
            .expect("development admits an inline secret");
        assert_eq!(
            Some("postgres://pix.invalid/pix"),
            kept.as_ref().map(Secret::expose)
        );
    }

    #[test]
    fn a_secret_file_is_read_trimmed_and_refused_when_empty_missing_or_doubled() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let full = directory.path().join("url");
        std::fs::write(&full, "  postgres://pix.invalid/pix\n").expect("the fixture is written");
        let read_back =
            read("db.url", None, Some(&full), Profile::Production).expect("the file is read");
        assert_eq!(
            Some("postgres://pix.invalid/pix"),
            read_back.as_ref().map(Secret::expose)
        );
        let blank = directory.path().join("blank");
        std::fs::write(&blank, " \n\t").expect("the fixture is written");
        let error = read("db.url", None, Some(&blank), Profile::Production)
            .expect_err("a blank secret file is refused");
        assert!(
            matches!(&error, Error::EmptySecret { key, .. } if key == "db.url_file"),
            "{error:?}"
        );
        let missing = directory.path().join("missing");
        let error = read("db.url", None, Some(&missing), Profile::Production)
            .expect_err("a missing secret file is refused");
        assert!(matches!(error, Error::SecretFile { .. }), "{error:?}");
        let inline = Secret::new("x");
        let error = read("db.url", Some(&inline), Some(&full), Profile::Development)
            .expect_err("a value and its file exclude each other");
        assert!(matches!(error, Error::SecretConflict { .. }), "{error:?}");
    }
}
