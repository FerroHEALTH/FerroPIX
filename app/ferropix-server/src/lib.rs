// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FerroPIX server library: the run path the `ferropix` binary and its
//! integration tests share.
//!
//! [`cli`] parses the command line, [`config`] reads the file and the
//! environment into one [`config::settings::Settings`], [`telemetry`]
//! installs the subscriber, and [`http`] builds the routes under `{base}` and
//! serves them until the process is asked to stop. `main.rs` only hands in
//! the arguments and returns the exit code.

pub mod auth;
pub mod cli;
pub mod config;
pub mod fhir;
pub mod healthcheck;
pub mod http;
pub mod telemetry;

use std::error::Error;
use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use tokio::net::TcpListener;

use crate::cli::{Cli, Command, ConfigCommand};
use crate::config::Config;
use crate::config::settings::Settings;

/// The exit code of a job that parses and does not run yet: the conventional
/// usage exit.
pub const EXIT_PENDING_JOB: u8 = 2;

/// The exit code of a refused configuration, `EX_CONFIG` from `sysexits`
/// (<https://man.freebsd.org/cgi/man.cgi?query=sysexits>), so an
/// orchestrator tells a bad configuration from a failure to serve.
pub const EXIT_CONFIG: u8 = 78;

/// Runs the binary with `args` and returns the process exit code.
///
/// `args` is the whole argument vector, the program name included, so the
/// command-line parser reports usage under the right name.
#[must_use]
#[expect(
    clippy::print_stderr,
    reason = "a pending job and a refused configuration are reported before any log subscriber exists"
)]
pub fn run<I>(args: I) -> ExitCode
where
    I: IntoIterator<Item = String>,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            // NOTE: no specification governs this: our own design; clap prints
            // the usage or the version itself and names the exit code.
            let code = error.exit_code();
            if error.print().is_err() {
                return ExitCode::FAILURE;
            }
            return u8::try_from(code).map_or(ExitCode::FAILURE, ExitCode::from);
        }
    };
    if let Some(issue) = cli.command.pending_issue() {
        eprintln!(
            "ferropix: `{}` is not implemented yet; it lands with issue #{issue}",
            cli.command.spelling()
        );
        return ExitCode::from(EXIT_PENDING_JOB);
    }
    let settings = match Config::load(cli.config.as_deref()).and_then(|config| config.resolve()) {
        Ok(settings) => settings,
        Err(error) if cli.command == Command::Healthcheck => {
            eprintln!("ferropix: not up: {}", chain(&error));
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("ferropix: the configuration is refused: {}", chain(&error));
            return ExitCode::from(EXIT_CONFIG);
        }
    };
    match cli.command {
        Command::Config {
            command: ConfigCommand::Check,
        } => config_check(&settings),
        Command::Healthcheck => runtime_then(|| healthcheck(settings)),
        Command::Serve => serve(settings),
        Command::Db { .. } => ExitCode::from(EXIT_PENDING_JOB),
    }
}

/// Runs `config check`: the settings resolved, and nothing bound.
#[expect(
    clippy::print_stdout,
    reason = "the job's one answer goes to standard output"
)]
fn config_check(settings: &Settings) -> ExitCode {
    println!(
        "ferropix: the configuration is accepted ({} profile, {} seeded domain(s))",
        match settings.profile {
            config::Profile::Production => "production",
            config::Profile::Development => "development",
        },
        settings.domains.len()
    );
    ExitCode::SUCCESS
}

/// Runs `healthcheck` and reports the outcome as one line.
#[expect(
    clippy::print_stdout,
    reason = "the job's one answer goes to standard output"
)]
async fn healthcheck(settings: Settings) -> ExitCode {
    let target = healthcheck::target(settings.server.listen);
    let outcome =
        healthcheck::check(target, &settings.server.base_path, healthcheck::TIMEOUT).await;
    println!("ferropix: {outcome}");
    if outcome.is_up() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Runs `serve`: the subscriber installed, the listener bound, and the
/// routes served until `SIGTERM` or `SIGINT` and the bounded drain.
#[expect(
    clippy::print_stderr,
    reason = "a subscriber that cannot be installed is reported on stderr"
)]
fn serve(settings: Settings) -> ExitCode {
    if let Err(error) = telemetry::init(&settings.telemetry, std::io::stdout().is_terminal()) {
        eprintln!("ferropix: cannot start: {}", chain(&error));
        return ExitCode::from(EXIT_CONFIG);
    }
    runtime_then(|| async move {
        match serve_settings(settings).await {
            Ok(()) => {
                tracing::info!("ferropix stopped");
                ExitCode::SUCCESS
            }
            Err(error) => {
                tracing::error!(error = %chain(&error), "cannot serve");
                ExitCode::FAILURE
            }
        }
    })
}

/// Why `serve` could not run.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServeError {
    /// The resource server could not start.
    #[error("the resource server could not start")]
    Auth(#[source] auth::SmartError),
    /// The listener could not be bound, or serving failed.
    #[error("the listener could not serve")]
    Io(#[source] std::io::Error),
}

/// Reads the authorization servers, binds the listener `settings` names and
/// serves until the process is asked to stop.
///
/// # Errors
/// Returns [`ServeError::Auth`] when an authorization server cannot be read
/// and [`ServeError::Io`] for the I/O error from binding, accepting or
/// serving.
pub async fn serve_settings(settings: Settings) -> Result<(), ServeError> {
    // TODO(#59): apply the pending migrations unless [db] migrate = false, and seed the domains.
    let Settings {
        server,
        auth,
        domains,
        pixm,
        pdqm,
        pmir,
        operator,
        ..
    } = settings;
    let smart = Arc::new(
        auth::Smart::discover(auth, &domains)
            .await
            .map_err(ServeError::Auth)?,
    );
    let mounts = fhir::Mounts {
        pixm: pixm.enabled,
        remove_patient: pixm.remove_patient,
        pdqm: pdqm.enabled,
        pmir: pmir.enabled,
        operator: operator.enabled,
    };
    let guarded = fhir::routes(&server.base_path, &server.public_base_url, mounts, &smart);
    let listener = TcpListener::bind(server.listen)
        .await
        .map_err(ServeError::Io)?;
    tracing::info!(
        listen = %listener.local_addr().map_err(ServeError::Io)?,
        base_path = server.base_path.as_str(),
        version = env!("CARGO_PKG_VERSION"),
        "ferropix serving"
    );
    http::serve_until(
        listener,
        http::router(&server, Some(guarded)),
        server.drain_timeout,
        http::shutdown_signal(),
    )
    .await
    .map_err(ServeError::Io)
}

/// Runs `job` on a multi-threaded runtime.
#[expect(
    clippy::print_stderr,
    reason = "a runtime that cannot start is reported before any log subscriber exists"
)]
fn runtime_then<F, J>(job: J) -> ExitCode
where
    J: FnOnce() -> F,
    F: Future<Output = ExitCode>,
{
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(job()),
        Err(error) => {
            eprintln!("ferropix: cannot start the runtime: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Renders `error` with every cause in its chain, colon-separated.
#[must_use]
pub fn chain(error: &dyn Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        // NOTE: no specification governs this: our own design; writing to a
        // String cannot fail, so the result carries nothing.
        if write!(text, ": {cause}").is_err() {
            break;
        }
        source = cause.source();
    }
    text
}
