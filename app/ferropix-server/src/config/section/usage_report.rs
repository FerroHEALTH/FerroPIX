// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[usage_report]` section: the installation report to `FerroPULSE`.
//!
//! The report is on by default, kept off by `enabled = false`, an unset
//! `state_dir` or a CI environment, which
//! [`crate::usage_report::reporter::prepare`] decides. No specification
//! governs this: our own design, after the siblings' reports.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::error::Error;
use crate::config::section::{is_loopback_url, url};

/// How the server was deployed, as the deployment artifact states it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Deployment {
    /// A Docker Compose file.
    Compose,
    /// A Helm chart.
    Helm,
    /// The plain binary.
    Binary,
    /// Not stated.
    #[default]
    Unknown,
}

/// The `[usage_report]` section as written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct UsageReport {
    /// Whether the report is sent.
    pub enabled: bool,
    /// The collector the report is posted to.
    pub endpoint: String,
    /// The writable directory the instance id lives in; unset keeps the report
    /// off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_dir: Option<PathBuf>,
    /// How the server was deployed.
    pub deployment: Deployment,
}

impl Default for UsageReport {
    fn default() -> Self {
        Self {
            enabled: true,
            endpoint: String::from("https://report.ferropulse.eu/v1/report"),
            state_dir: None,
            deployment: Deployment::Unknown,
        }
    }
}

/// The `[usage_report]` section, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageReportSettings {
    /// Whether the deployment left the report on.
    pub enabled: bool,
    /// The collector.
    pub endpoint: url::Url,
    /// The writable directory the instance id lives in.
    pub state_dir: Option<PathBuf>,
    /// How the server was deployed.
    pub deployment: Deployment,
}

impl UsageReport {
    /// Resolves the section.
    ///
    /// # Errors
    /// Returns [`Error::Url`] for an endpoint that does not parse and
    /// [`Error::Value`] for one that carries credentials or is plain `http`
    /// to a host other than a loopback one.
    pub fn resolve(&self) -> Result<UsageReportSettings, Error> {
        let endpoint = url("usage_report.endpoint", &self.endpoint)?;
        let secure = match endpoint.scheme() {
            "https" => endpoint.host().is_some(),
            "http" => is_loopback_url(&endpoint),
            _ => false,
        };
        if !secure || !endpoint.username().is_empty() || endpoint.password().is_some() {
            return Err(Error::Value {
                key: String::from("usage_report.endpoint"),
                rule: "https, or http to a loopback host, without credentials",
            });
        }
        Ok(UsageReportSettings {
            enabled: self.enabled,
            endpoint,
            state_dir: self.state_dir.clone(),
            deployment: self.deployment,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::UsageReport;
    use crate::config::error::Error;
    use crate::usage_report::reporter::{Off, prepare};

    #[test]
    fn the_report_is_off_without_a_state_directory_or_when_disabled() {
        let default = UsageReport::default().resolve().expect("resolves");
        assert!(
            matches!(prepare(&default, None), Err(Off::NoStateDir)),
            "no state_dir keeps it off"
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let on = UsageReport {
            state_dir: Some(dir.path().to_path_buf()),
            ..UsageReport::default()
        };
        assert!(prepare(&on.resolve().expect("resolves"), None).is_ok());
        let off = UsageReport {
            enabled: false,
            ..on
        };
        assert!(matches!(
            prepare(&off.resolve().expect("resolves"), None),
            Err(Off::Switch)
        ));
    }

    #[test]
    fn an_endpoint_in_clear_text_or_with_credentials_is_refused() {
        for bad in [
            "http://report.example.org/v1/report",
            "https://u:p@report.example.org/v1/report",
            "ftp://report.example.org",
        ] {
            let report = UsageReport {
                endpoint: bad.to_owned(),
                ..UsageReport::default()
            };
            assert!(
                matches!(report.resolve(), Err(Error::Value { .. })),
                "{bad}"
            );
        }
    }
}
