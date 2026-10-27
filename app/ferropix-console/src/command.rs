// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The run path of the `ferropix-console` binary: the command line parsed,
//! the configuration read, and each job run to its exit code.

use std::error::Error;
use std::fmt::Write as _;
use std::process::ExitCode;

use clap::Parser;
use ferrohealth_config::loader::Loaded;
use ferrohealth_server::cli::{Common, ConfigCommand};
use ferrohealth_telemetry::section::TelemetrySection;
use ferrohealth_telemetry::telemetry::{Console, Service, Telemetry};

use crate::cli::Cli;
use crate::config::Config;
use crate::config::settings::Settings;
use crate::server::ConsoleState;

/// How long `healthcheck` waits for the readiness answer, shorter than the
/// five seconds a container `HEALTHCHECK` commonly allows.
pub const HEALTHCHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// The exit code of a configuration the console will not start on, from the
/// BSD `sysexits.h` set (`EX_CONFIG`).
pub const EXIT_CONFIG: u8 = 78;

/// Runs the binary with `args` and returns the process exit code.
///
/// `args` is the whole argument vector, the program name included, so the
/// command-line parser reports usage under the right name.
#[must_use]
#[expect(
    clippy::print_stderr,
    reason = "a refused configuration is reported before any log subscriber exists"
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
    let loaded = Config::load(&cli.config).and_then(|loaded| {
        loaded
            .settings()
            .resolve()
            .map(|settings| (loaded, settings))
    });
    let (loaded, settings) = match loaded {
        Ok(loaded) => loaded,
        Err(error) if cli.command == Common::Healthcheck => {
            eprintln!("ferropix-console: not up: {}", chain(&error));
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("ferropix-console: cannot start: {}", chain(&error));
            return ExitCode::from(EXIT_CONFIG);
        }
    };
    match cli.command {
        Common::Healthcheck => runtime_then(|| healthcheck(&settings)),
        Common::Config {
            command: ConfigCommand::Check,
        } => config_check(&loaded, settings),
        Common::Serve => {
            let telemetry = loaded.settings().telemetry.clone();
            runtime_then(|| serve(settings, telemetry))
        }
    }
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
            eprintln!("ferropix-console: cannot start the runtime: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs `config check`: every key printed with where it came from and every
/// secret masked, the state the server would start with built, and nothing
/// bound.
#[expect(
    clippy::print_stdout,
    reason = "the job's one answer goes to standard output"
)]
#[expect(
    clippy::print_stderr,
    reason = "the job's refusal goes to standard error"
)]
fn config_check(loaded: &Loaded<Config>, settings: Settings) -> ExitCode {
    match ferrohealth_config::report::check_report(loaded) {
        Ok(report) => println!("{report}"),
        Err(error) => {
            eprintln!(
                "ferropix-console: cannot show the configuration: {}",
                chain(&error)
            );
            return ExitCode::from(EXIT_CONFIG);
        }
    }
    match ConsoleState::new(settings) {
        Ok(_state) => {
            println!("ferropix-console: the configuration is valid");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("ferropix-console: cannot start: {}", chain(&error));
            ExitCode::from(EXIT_CONFIG)
        }
    }
}

/// Runs `healthcheck` against the console this configuration describes.
#[expect(
    clippy::print_stdout,
    reason = "the job's one-line answer goes to standard output"
)]
#[expect(
    clippy::print_stderr,
    reason = "the job's one-line refusal goes to standard error"
)]
async fn healthcheck(settings: &Settings) -> ExitCode {
    let target = ferrohealth_server::healthcheck::target(settings.listen);
    let outcome = ferrohealth_server::healthcheck::check(target, "", HEALTHCHECK_TIMEOUT).await;
    if outcome.exit_code() == 0 {
        println!("ferropix-console: {outcome}");
        ExitCode::SUCCESS
    } else {
        eprintln!("ferropix-console: {outcome}");
        ExitCode::FAILURE
    }
}

/// Runs `serve`: the subscriber, the state, and the server until `SIGTERM`
/// or `SIGINT`.
#[expect(
    clippy::print_stderr,
    reason = "a refusal before the subscriber starts has no other place to go"
)]
async fn serve(settings: Settings, section: TelemetrySection) -> ExitCode {
    let service = Service::new("ferropix-console", env!("CARGO_PKG_VERSION"))
        .export_crate("ferropix_console");
    let telemetry = match Telemetry::new(&section, service) {
        Ok(telemetry) => telemetry,
        Err(error) => {
            eprintln!("ferropix-console: cannot start the log: {}", chain(&error));
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = telemetry.install(&section, &Console::detect()) {
        eprintln!("ferropix-console: cannot start the log: {}", chain(&error));
        return ExitCode::FAILURE;
    }
    ferrohealth_telemetry::panic::install_hook();
    let code = listen_and_serve(settings).await;
    if let Err(error) = telemetry.shutdown() {
        tracing::warn!(error = %chain(&error), "the telemetry did not flush");
    }
    code
}

/// Builds the state `settings` describe and serves it until `SIGTERM` or
/// `SIGINT`.
async fn listen_and_serve(settings: Settings) -> ExitCode {
    let listen = settings.listen;
    let state = match ConsoleState::new(settings) {
        Ok(state) => state,
        Err(error) => {
            tracing::error!(error = %chain(&error), "cannot start");
            return ExitCode::from(EXIT_CONFIG);
        }
    };
    let listener = match tokio::net::TcpListener::bind(listen).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%listen, %error, "cannot bind the listen address");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(%listen, "the operator console is listening");
    let lifecycle = state.lifecycle().clone();
    let stop = ferrohealth_server::serve::shutdown_signal();
    match crate::server::serve(listener, crate::server::router(state), lifecycle, stop).await {
        Ok(_drained) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "the listener failed");
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
