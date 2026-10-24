// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `tracing` console: one subscriber for the process, its filter and
//! rendering from `[telemetry]`. No specification governs this: our own
//! design.

use crate::config::section::telemetry::{Format, TelemetrySettings};

/// How the console renders, once `auto` is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rendering {
    /// One JSON object per line.
    Json,
    /// Human-readable lines.
    Pretty,
}

/// Returns the rendering `format` takes when stdout is or is not a terminal.
#[must_use]
pub const fn rendering(format: Format, stdout_is_terminal: bool) -> Rendering {
    match format {
        Format::Pretty => Rendering::Pretty,
        Format::Auto if stdout_is_terminal => Rendering::Pretty,
        Format::Json | Format::Auto => Rendering::Json,
    }
}

/// Why the subscriber could not be installed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum InitError {
    /// The filter does not parse.
    #[error("telemetry.filter is not a tracing filter directive")]
    Filter(#[source] tracing_subscriber::filter::ParseError),
    /// A global subscriber is already installed.
    #[error("a tracing subscriber is already installed")]
    Installed(#[source] tracing_subscriber::util::TryInitError),
}

/// Installs the process's subscriber under `settings`.
///
/// # Errors
/// Returns [`InitError::Filter`] for a filter that does not parse and
/// [`InitError::Installed`] when a subscriber is already installed.
pub fn init(settings: &TelemetrySettings, stdout_is_terminal: bool) -> Result<(), InitError> {
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    let filter =
        tracing_subscriber::EnvFilter::try_new(&settings.filter).map_err(InitError::Filter)?;
    let registry = tracing_subscriber::registry().with(filter);
    let installed = match rendering(settings.format, stdout_is_terminal) {
        Rendering::Json => registry
            .with(tracing_subscriber::fmt::layer().json().flatten_event(true))
            .try_init(),
        Rendering::Pretty => registry
            .with(tracing_subscriber::fmt::layer().with_ansi(stdout_is_terminal))
            .try_init(),
    };
    installed.map_err(InitError::Installed)?;
    if settings.otlp_endpoint.is_some() {
        // TODO(#143): export the spans to the OTLP endpoint.
        tracing::warn!("telemetry.otlp_endpoint is set, and no span is exported to it yet");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Rendering, rendering};
    use crate::config::section::telemetry::Format;

    #[test]
    fn auto_renders_pretty_on_a_terminal_and_json_elsewhere() {
        assert_eq!(Rendering::Pretty, rendering(Format::Auto, true));
        assert_eq!(Rendering::Json, rendering(Format::Auto, false));
        assert_eq!(Rendering::Json, rendering(Format::Json, true));
        assert_eq!(Rendering::Pretty, rendering(Format::Pretty, false));
    }
}
