// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[telemetry]` section: the `tracing` filter and format, and the OTLP
//! endpoint. No specification governs this: our own design.

use serde::{Deserialize, Serialize};

use crate::config::error::Error;
use crate::config::section::url;

/// How the console renders a log line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// `pretty` on a terminal, `json` otherwise.
    #[default]
    Auto,
    /// One JSON object per line.
    Json,
    /// Human-readable lines.
    Pretty,
}

/// The `[telemetry]` section as written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Telemetry {
    /// The `tracing` filter directive (`tracing_subscriber::EnvFilter`).
    pub filter: String,
    /// How a log line is rendered.
    pub format: Format,
    /// The OTLP collector the spans are exported to; empty for none.
    pub otlp_endpoint: String,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            filter: String::from("info"),
            format: Format::Auto,
            otlp_endpoint: String::new(),
        }
    }
}

/// The `[telemetry]` section, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetrySettings {
    /// The `tracing` filter directive.
    pub filter: String,
    /// How a log line is rendered.
    pub format: Format,
    /// The OTLP collector, when one is named.
    pub otlp_endpoint: Option<url::Url>,
}

impl Telemetry {
    /// Resolves the section.
    ///
    /// # Errors
    /// Returns [`Error::Value`] for a filter `tracing-subscriber` refuses or
    /// an endpoint that is not `http` or `https`, and [`Error::Url`] for one
    /// that does not parse.
    pub fn resolve(&self) -> Result<TelemetrySettings, Error> {
        if tracing_subscriber::EnvFilter::try_new(&self.filter).is_err() {
            return Err(Error::Value {
                key: String::from("telemetry.filter"),
                rule: "a tracing-subscriber EnvFilter directive",
            });
        }
        let otlp_endpoint = if self.otlp_endpoint.is_empty() {
            None
        } else {
            let endpoint = url("telemetry.otlp_endpoint", &self.otlp_endpoint)?;
            if !matches!(endpoint.scheme(), "http" | "https") {
                return Err(Error::Value {
                    key: String::from("telemetry.otlp_endpoint"),
                    rule: "an http or https URL",
                });
            }
            Some(endpoint)
        };
        Ok(TelemetrySettings {
            filter: self.filter.clone(),
            format: self.format,
            otlp_endpoint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Telemetry;
    use crate::config::error::Error;

    #[test]
    fn a_filter_tracing_refuses_and_an_endpoint_of_another_scheme_are_refused() {
        let bad = Telemetry {
            filter: String::from("info,[=x"),
            ..Telemetry::default()
        };
        assert!(matches!(bad.resolve(), Err(Error::Value { .. })));
        let bad = Telemetry {
            otlp_endpoint: String::from("grpc://collector.invalid:4317"),
            ..Telemetry::default()
        };
        assert!(matches!(bad.resolve(), Err(Error::Value { .. })));
        let good = Telemetry {
            otlp_endpoint: String::from("http://collector.invalid:4318"),
            ..Telemetry::default()
        };
        assert!(
            good.resolve()
                .expect("an http endpoint resolves")
                .otlp_endpoint
                .is_some()
        );
    }
}
