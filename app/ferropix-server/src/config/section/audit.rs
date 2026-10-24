// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[audit]` section: the Audit Record Repository the BALP records go
//! to, and the spool they wait in.
//!
//! BALP sends its `AuditEvent` over the ATX: FHIR Feed Option of ITI-20 (BALP
//! 1.1.4 §1:52.1.1.1), so the repository is a FHIR base. Every record names
//! the patient, so the base is `https`, and plain `http` is admitted under
//! the development profile alone. The table's shape follows FerroFED's
//! `[audit.repository]`; no specification governs it: our own design.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Profile;
use crate::config::error::Error;
use crate::config::section::url;

/// The `[audit]` section as written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Audit {
    /// The repository's FHIR base; empty for none.
    pub url: String,
    /// The directory the records wait in until they are delivered.
    pub spool_dir: PathBuf,
    /// The most bytes the spool holds.
    pub spool_max_bytes: u64,
    /// The most records the spool holds.
    pub spool_max_events: usize,
    /// The longest storing one record may take.
    pub spool_write_timeout_ms: u64,
    /// How long one delivery may take.
    pub timeout_ms: u64,
    /// The longest wait between two delivery attempts.
    pub retry_max_ms: u64,
}

impl Default for Audit {
    fn default() -> Self {
        Self {
            url: String::new(),
            spool_dir: PathBuf::from("/var/lib/ferropix/audit"),
            spool_max_bytes: 64 * 1024 * 1024,
            spool_max_events: 100_000,
            spool_write_timeout_ms: 2_000,
            timeout_ms: 5_000,
            retry_max_ms: 60_000,
        }
    }
}

/// The `[audit]` section, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditSettings {
    /// The repository's FHIR base, when one is named.
    pub url: Option<url::Url>,
    /// The directory the records wait in.
    pub spool_dir: PathBuf,
    /// The most bytes the spool holds.
    pub spool_max_bytes: u64,
    /// The most records the spool holds.
    pub spool_max_events: usize,
    /// The longest storing one record may take.
    pub spool_write_timeout: Duration,
    /// How long one delivery may take.
    pub timeout: Duration,
    /// The longest wait between two delivery attempts.
    pub retry_max: Duration,
}

impl Audit {
    /// Resolves the section under `profile`.
    ///
    /// # Errors
    /// Returns [`Error::Url`] for a base that does not parse, [`Error::Value`]
    /// for a base that is not `http` or `https` or a bound of 0, and
    /// [`Error::DevelopmentOnly`] for a plain `http` base under production.
    pub fn resolve(&self, profile: Profile) -> Result<AuditSettings, Error> {
        let base = if self.url.is_empty() {
            None
        } else {
            let base = url("audit.url", &self.url)?;
            match base.scheme() {
                "https" => {}
                "http" if profile == Profile::Development => {}
                "http" => {
                    return Err(Error::DevelopmentOnly {
                        key: String::from("audit.url"),
                        rule: "a plain http repository, while every record names the patient",
                    });
                }
                _ => return Err(value("audit.url", "an https FHIR base")),
            }
            Some(base)
        };
        if self.spool_dir.as_os_str().is_empty() {
            return Err(value("audit.spool_dir", "a directory"));
        }
        for (key, zero) in [
            ("audit.spool_max_bytes", self.spool_max_bytes == 0),
            ("audit.spool_max_events", self.spool_max_events == 0),
            (
                "audit.spool_write_timeout_ms",
                self.spool_write_timeout_ms == 0,
            ),
            ("audit.timeout_ms", self.timeout_ms == 0),
            ("audit.retry_max_ms", self.retry_max_ms == 0),
        ] {
            if zero {
                return Err(value(key, "it must be above 0"));
            }
        }
        Ok(AuditSettings {
            url: base,
            spool_dir: self.spool_dir.clone(),
            spool_max_bytes: self.spool_max_bytes,
            spool_max_events: self.spool_max_events,
            spool_write_timeout: Duration::from_millis(self.spool_write_timeout_ms),
            timeout: Duration::from_millis(self.timeout_ms),
            retry_max: Duration::from_millis(self.retry_max_ms),
        })
    }
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
    use super::Audit;
    use crate::config::Profile;
    use crate::config::error::Error;

    fn audit(url: &str) -> Audit {
        Audit {
            url: url.to_owned(),
            ..Audit::default()
        }
    }

    #[test]
    fn a_plain_http_repository_is_refused_under_production_only() {
        let error = audit("http://arr.example.org/fhir")
            .resolve(Profile::Production)
            .expect_err("every record names the patient");
        assert!(matches!(error, Error::DevelopmentOnly { .. }), "{error:?}");
        audit("http://arr.example.org/fhir")
            .resolve(Profile::Development)
            .expect("development admits plain http");
        audit("https://arr.example.org/fhir")
            .resolve(Profile::Production)
            .expect("https resolves");
    }

    #[test]
    fn a_repository_of_another_scheme_and_a_zero_bound_are_refused() {
        assert!(matches!(
            audit("tcp://arr.example.org:514").resolve(Profile::Development),
            Err(Error::Value { .. })
        ));
        let zero = Audit {
            spool_max_events: 0,
            ..Audit::default()
        };
        assert!(matches!(
            zero.resolve(Profile::Production),
            Err(Error::Value { .. })
        ));
    }
}
