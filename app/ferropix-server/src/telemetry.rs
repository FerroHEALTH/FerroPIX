// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `tracing` console and the span export: one subscriber for the
//! process, its filter and rendering from `[telemetry]`.
//!
//! When `telemetry.otlp_endpoint` names a collector, `serve` exports the
//! server's own spans to it as OpenTelemetry traces over OTLP/gRPC
//! ([`Traces`]). Only spans leave ([`exported`]): no `tracing` event, and no
//! span of another crate. The one span is the request span of the request
//! log, whose fields are the method, the matched route template and the
//! status, never a raw path, a query string, a body, a header value or an
//! identifier. The console filter decides what is logged and never what is
//! traced. No specification governs this: our own design, after FerroFED's
//! trace export.

use std::io;

use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_otlp::WithTonicConfig as _;
use opentelemetry_otlp::tonic_types::transport::ClientTlsConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use tracing::{Metadata, Subscriber};
use tracing_subscriber::filter::{EnvFilter, FilterFn};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Layer, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt as _;

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

/// Why the subscriber or the span export could not be built, installed or
/// stopped.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum InitError {
    /// The filter does not parse.
    #[error("telemetry.filter is not a tracing filter directive")]
    Filter(#[source] tracing_subscriber::filter::ParseError),
    /// A global subscriber is already installed.
    #[error("a tracing subscriber is already installed")]
    Installed(#[source] tracing_subscriber::util::TryInitError),
    /// The OTLP span exporter could not be built for `telemetry.otlp_endpoint`.
    #[error("the OTLP span exporter could not be built")]
    Exporter(#[source] opentelemetry_otlp::ExporterBuildError),
    /// The tracer provider could not flush or stop its exporter.
    #[error("the OTLP span export could not shut down")]
    Shutdown(#[source] opentelemetry_sdk::error::OTelSdkError),
}

/// The name every exported span is described by, and the tracer's scope.
pub const SERVICE: &str = "ferropix";

/// The span export: one tracer provider that sends the server's spans to an
/// OTLP collector in batches.
pub struct Traces {
    provider: SdkTracerProvider,
}

impl Traces {
    /// Returns the export to the OTLP/gRPC collector at `endpoint`, with TLS
    /// over the `WebPKI` roots for an `https` endpoint.
    ///
    /// The exporter speaks gRPC through `tonic`, so this runs inside the
    /// Tokio runtime that carries the export.
    ///
    /// # Errors
    /// Returns [`InitError::Exporter`] when the exporter cannot be built.
    pub fn new(endpoint: &url::Url) -> Result<Self, InitError> {
        let mut builder = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint.as_str());
        if endpoint.scheme() == "https" {
            builder = builder.with_tls_config(ClientTlsConfig::new().with_webpki_roots());
        }
        let exporter = builder.build().map_err(InitError::Exporter)?;
        let resource = Resource::builder()
            .with_service_name(SERVICE)
            .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
            .build();
        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(resource)
            .build();
        Ok(Self { provider })
    }

    /// Returns the tracer the [`spans`] layer records through.
    #[must_use]
    pub fn tracer(&self) -> SdkTracer {
        self.provider.tracer(SERVICE)
    }

    /// Flushes every span still held and stops the exporter.
    ///
    /// # Errors
    /// Returns [`InitError::Shutdown`] when the exporter cannot flush or
    /// stop.
    pub fn shutdown(&self) -> Result<(), InitError> {
        self.provider.shutdown().map_err(InitError::Shutdown)
    }
}

impl std::fmt::Debug for Traces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Traces").finish_non_exhaustive()
    }
}

/// Whether the span export sees `metadata`: a span, never an event, of one
/// of FerroPIX's own crates.
#[must_use]
pub fn exported(metadata: &Metadata<'_>) -> bool {
    // NOTE: no specification governs this: our own design; an event may carry
    // what a span never does, so only spans leave the process.
    metadata.is_span()
        && metadata
            .target()
            .split("::")
            .next()
            .is_some_and(|krate| krate.starts_with("ferropix_"))
}

/// Returns the layer that exports the spans [`exported`] admits through
/// `tracer`, with no thread and no source location.
pub fn spans<S>(tracer: SdkTracer) -> impl Layer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_threads(false)
        .with_location(false)
        .with_filter(FilterFn::new(exported))
}

/// Builds the subscriber under `settings` for `rendering`, writing through
/// `writer` with colour when `ansi`, and exporting spans through `tracer`
/// when one is given.
///
/// The filter decides what the console writes and nothing else, so a
/// quieter log never thins a trace.
///
/// # Errors
/// Returns [`InitError::Filter`] for a filter that does not parse.
pub fn subscriber<W>(
    settings: &TelemetrySettings,
    rendering: Rendering,
    ansi: bool,
    writer: W,
    tracer: Option<SdkTracer>,
) -> Result<impl Subscriber + Send + Sync + use<W>, InitError>
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    let filter = EnvFilter::try_new(&settings.filter).map_err(InitError::Filter)?;
    let console: Box<dyn Layer<tracing_subscriber::Registry> + Send + Sync> = match rendering {
        Rendering::Json => Box::new(
            tracing_subscriber::fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(false)
                .with_span_list(false)
                .with_writer(writer),
        ),
        Rendering::Pretty => Box::new(
            tracing_subscriber::fmt::layer()
                .with_ansi(ansi)
                .with_writer(writer),
        ),
    };
    Ok(tracing_subscriber::registry()
        .with(console.with_filter(filter))
        .with(tracer.map(spans)))
}

/// Installs the process's subscriber under `settings` on stdout, exporting
/// spans through `tracer` when one is given.
///
/// # Errors
/// Returns [`InitError::Filter`] for a filter that does not parse and
/// [`InitError::Installed`] when a subscriber is already installed.
pub fn init(
    settings: &TelemetrySettings,
    stdout_is_terminal: bool,
    tracer: Option<SdkTracer>,
) -> Result<(), InitError> {
    subscriber(
        settings,
        rendering(settings.format, stdout_is_terminal),
        stdout_is_terminal,
        io::stdout,
        tracer,
    )?
    .try_init()
    .map_err(InitError::Installed)
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
