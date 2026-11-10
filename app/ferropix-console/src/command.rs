// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The run path of the `ferropix-console` binary: the command line parsed,
//! the configuration read, and each job run to its exit code.

use std::process::ExitCode;

use ferrohealth_config::loader::Loaded;
use ferrohealth_server::cli::{Common, ConfigCommand};
use ferrohealth_server::run::{Failure, Run, chain};
use ferrohealth_telemetry::section::TelemetrySection;
use ferrohealth_telemetry::telemetry::{Console, InitError, Service, Telemetry};

use crate::cli::Cli;
use crate::config::Config;
use crate::config::settings::Settings;
use crate::server::ConsoleState;

/// How long `healthcheck` waits for the readiness answer, shorter than the
/// five seconds a container `HEALTHCHECK` commonly allows.
pub const HEALTHCHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// The run path of the `ferropix-console` binary, which names it in every
/// line it writes.
pub const RUN: Run = Run::new("ferropix-console");

/// Runs the job `cli` names on the runtime of [`RUN`] and returns its exit
/// code.
///
/// # Errors
///
/// Returns the [`Failure`] of a configuration that does not resolve (with
/// [`EXIT_CONFIG`](ferrohealth_server::run::EXIT_CONFIG), or exit 1 for `healthcheck`) and of a job that fails.
pub async fn dispatch(cli: Cli) -> Result<ExitCode, Failure> {
    let loaded = Config::load(&cli.config).and_then(|loaded| {
        loaded
            .settings()
            .resolve()
            .map(|settings| (loaded, settings))
    });
    let (loaded, settings) = loaded.map_err(|error| cli.command.refused(error))?;
    match cli.command {
        Common::Healthcheck => healthcheck(&settings).await,
        Common::Config {
            command: ConfigCommand::Check,
        } => config_check(&loaded, settings).await,
        Common::Serve => {
            let telemetry = loaded.settings().telemetry.clone();
            Box::pin(serve(settings, telemetry)).await
        }
    }
}

/// Runs `config check`: every key printed with where it came from and every
/// secret masked, the state the server would start with built, and nothing
/// bound.
///
/// The check calls no other server: a console whose `[oidc]` leaves the
/// provider's endpoints to discovery builds its client of the FerroPIX
/// server alone, and its relying party when it starts.
#[expect(
    clippy::print_stdout,
    reason = "the job's one answer goes to standard output"
)]
async fn config_check(loaded: &Loaded<Config>, settings: Settings) -> Result<ExitCode, Failure> {
    ferrohealth_server::run::config_check(loaded)?;
    let built = if settings
        .oidc
        .as_ref()
        .is_none_or(crate::config::settings::OidcSettings::endpoints_given)
    {
        ConsoleState::new(settings).await.map(drop)
    } else {
        crate::pix::Pix::new(&settings.pix)
            .map(drop)
            .map_err(crate::server::StateError::from)
    };
    built.map_err(Failure::refused_configuration)?;
    println!("ferropix-console: the configuration is valid");
    Ok(ExitCode::SUCCESS)
}

/// Runs `healthcheck` against the console this configuration describes.
#[expect(
    clippy::print_stdout,
    reason = "the job's one-line answer goes to standard output"
)]
async fn healthcheck(settings: &Settings) -> Result<ExitCode, Failure> {
    let target = ferrohealth_server::healthcheck::target(settings.listen);
    let outcome = ferrohealth_server::healthcheck::check(target, "", HEALTHCHECK_TIMEOUT).await;
    if outcome.exit_code() == 0 {
        println!("ferropix-console: {outcome}");
        Ok(ExitCode::SUCCESS)
    } else {
        Err(Failure::boxed(outcome.to_string()))
    }
}

/// Runs `serve`: the subscriber, the state, and the server until `SIGTERM`
/// or `SIGINT`.
async fn serve(settings: Settings, section: TelemetrySection) -> Result<ExitCode, Failure> {
    let service = Service::new("ferropix-console", env!("CARGO_PKG_VERSION"))
        .export_crate("ferropix_console");
    let log = |error: InitError| Failure::from(error).with_context("cannot start the log");
    let telemetry = Telemetry::new(&section, service).map_err(log)?;
    telemetry
        .install(&section, &Console::detect())
        .map_err(log)?;
    ferrohealth_telemetry::panic::install_hook();
    let served = listen_and_serve(settings).await;
    if let Err(error) = telemetry.shutdown() {
        tracing::warn!(error = %chain(&error), "the telemetry did not flush");
    }
    served
}

/// Builds the state `settings` describe and serves it until `SIGTERM` or
/// `SIGINT`.
async fn listen_and_serve(settings: Settings) -> Result<ExitCode, Failure> {
    let listen = settings.listen;
    let state = ConsoleState::new(settings)
        .await
        .map_err(Failure::refused_configuration)?;
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|error| Failure::from(error).with_context("cannot bind the listen address"))?;
    tracing::info!(%listen, "the operator console is listening");
    let lifecycle = state.lifecycle().clone();
    let stop = ferrohealth_server::serve::shutdown_signal();
    match crate::server::serve(listener, crate::server::router(state), lifecycle, stop).await {
        Ok(_drained) => Ok(ExitCode::SUCCESS),
        Err(error) => {
            tracing::error!(%error, "the listener failed");
            Ok(ExitCode::FAILURE)
        }
    }
}
