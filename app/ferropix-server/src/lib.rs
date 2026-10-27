// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FerroPIX server library: the run path the `ferropix` binary and its
//! integration tests share.
//!
//! [`cli`] parses the command line, [`config`] reads the file and the
//! environment into one [`config::settings::Settings`],
//! `ferrohealth-telemetry` installs the console, the export and the panic
//! hook, and [`http`] builds the routes under `{base}` and serves them until
//! the process is asked to stop. `main.rs` only hands in the arguments and
//! returns the exit code.

pub mod audit;
pub mod auth;
pub mod cli;
pub mod config;
pub mod feed;
pub mod fhir;
pub mod hl7v2;
pub mod http;
pub mod matching;
pub mod operator;
pub mod outbox;
pub mod pdqm;
pub mod serve;
pub mod store;
pub mod usage_report;
pub mod xcpd;

use std::error::Error;
use std::fmt::Write as _;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use ferrohealth_config::loader::Loaded;
use ferrohealth_server::cli::{Common, ConfigCommand};
use ferrohealth_server::healthcheck::Outcome;
use ferrohealth_server::section::ServerSection;
use ferrohealth_telemetry::section::TelemetrySection;
use ferrohealth_telemetry::telemetry::{Console, InitError, Service, Telemetry};

use crate::cli::{Cli, Command, DbCommand, MatchingCommand};
use crate::config::Config;
use crate::config::settings::Settings;

/// The scope of the meter the request log records
/// `http.server.request.duration` in.
pub const METER: &str = "ferropix_server";

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
    let loaded = Config::load(&cli.config).and_then(|loaded| {
        let mut settings = loaded.settings().resolve()?;
        settings.provenance = loaded.provenance().clone();
        Ok((loaded, settings))
    });
    let (loaded, settings) = match loaded {
        Ok(loaded) => loaded,
        Err(error) if cli.command == Command::Common(Common::Healthcheck) => {
            eprintln!("ferropix: not up: {}", chain(&error));
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("ferropix: the configuration is refused: {}", chain(&error));
            return ExitCode::from(EXIT_CONFIG);
        }
    };
    match cli.command {
        Command::Common(Common::Config {
            command: ConfigCommand::Check,
        }) => config_check(&loaded, &settings),
        Command::Common(Common::Healthcheck) => runtime_then(|| healthcheck(settings)),
        Command::Common(Common::Serve) => serve(settings),
        Command::Db { command } => db(settings, &command),
        Command::Matching {
            command: MatchingCommand::Estimate,
        } => estimate(settings),
    }
}

/// Runs `matching estimate`: the `u` of every field enough records state,
/// printed as the `[matching.fields]` tables that would set it, each beside
/// the `m` the configuration holds.
#[expect(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "the job prints its tables on standard output and a failure on stderr"
)]
fn estimate(settings: Settings) -> ExitCode {
    runtime_then(|| async move {
        let telemetry = match telemetry(&settings.telemetry) {
            Ok(telemetry) => telemetry,
            Err(error) => {
                eprintln!("ferropix: cannot start: {}", chain(&error));
                return ExitCode::from(EXIT_CONFIG);
            }
        };
        let outcome = async {
            let store = store::Store::connect(&settings.db).await?;
            store::key::verify(&store).await?;
            let estimates = store::estimate::estimate(&store).await;
            store.close().await;
            estimates
        }
        .await;
        let code = match outcome {
            Ok(estimates) => {
                println!("{}", estimate_tables(&settings.matching, &estimates));
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("ferropix: the estimate failed: {}", chain(&error));
                ExitCode::FAILURE
            }
        };
        shut_down(&telemetry);
        code
    })
}

/// Builds the telemetry `section` asks for, described as the `ferropix`
/// service, and installs its console, its span export and the panic hook.
///
/// With `[telemetry.otlp]` the exporters speak gRPC, so this runs inside the
/// Tokio runtime that carries the export.
///
/// # Errors
/// Returns the [`InitError`] of `ferrohealth-telemetry` when the providers
/// cannot be built or a subscriber is installed already.
pub fn telemetry(section: &TelemetrySection) -> Result<Arc<Telemetry>, InitError> {
    let telemetry = Telemetry::new(section, Service::new("ferropix", env!("CARGO_PKG_VERSION")))?;
    let _filter = telemetry.install(section, &Console::detect())?;
    ferrohealth_telemetry::panic::install_hook();
    Ok(Arc::new(telemetry))
}

/// Flushes and stops the span and metric export of `telemetry`.
fn shut_down(telemetry: &Telemetry) {
    if let Err(error) = telemetry.shutdown() {
        tracing::warn!(error = %chain(&error), "the telemetry did not flush");
    }
}

/// Writes `estimates` as `[matching.fields]` tables, each with the `m` of
/// `matching`; a field whose `u` reaches its `m` is named in a comment and
/// left out, since `[matching]` refuses it.
#[must_use]
pub fn estimate_tables(
    matching: &config::section::matching::Matching,
    estimates: &store::estimate::Estimates,
) -> String {
    let mut text = String::from("# Estimated by `ferropix matching estimate`.\n");
    for (field, (u, records)) in estimates {
        let m = matching
            .fields
            .get(field)
            .copied()
            .unwrap_or_else(|| matching::scorer::default_weights(*field))
            .m;
        if *u >= m {
            let _written = writeln!(
                text,
                "# {}: u {u:.6} from {records} records reaches m {m}; left out",
                field.key()
            );
            continue;
        }
        let _written = writeln!(
            text,
            "\n# from {records} records\n[matching.fields.{}]\nm = {m}\nu = {u:.6}",
            field.key()
        );
    }
    text
}

/// Runs `config check`: the settings resolved, every key printed with where
/// it came from and every secret masked, and nothing bound.
#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "the job's report goes to standard output and a refusal to stderr"
)]
fn config_check(loaded: &Loaded<Config>, settings: &Settings) -> ExitCode {
    match ferrohealth_config::report::check_report(loaded) {
        Ok(report) => println!("{report}"),
        Err(error) => {
            eprintln!(
                "ferropix: the configuration cannot be shown: {}",
                chain(&error)
            );
            return ExitCode::from(EXIT_CONFIG);
        }
    }
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
    let outcome = probe(&settings.server).await;
    println!("ferropix: {outcome}");
    if outcome.exit_code() == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// How long `healthcheck` waits for the readiness answer, shorter than the
/// five seconds a container `HEALTHCHECK` commonly allows.
pub const HEALTHCHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Asks the server `section` configures for `{base}/health/readiness`, over
/// the TLS `[server.tls]` configures, on the loopback when it listens on
/// every interface: the container's `HEALTHCHECK`.
pub async fn probe(section: &ServerSection) -> Outcome {
    use ferrohealth_server::healthcheck::{check, check_tls, target};

    let address = target(section.listen);
    let base = section.base_path.as_str();
    match &section.tls {
        None => check(address, base, HEALTHCHECK_TIMEOUT).await,
        Some(tls) => match ferrohealth_server::tls::HealthcheckTls::read(tls) {
            Ok(pinned) => check_tls(address, base, HEALTHCHECK_TIMEOUT, &pinned).await,
            Err(error) => Outcome::Failed(format!("the TLS files do not read: {error}")),
        },
    }
}

/// Runs `serve`: the subscriber installed, the listener bound, and the
/// routes served until `SIGTERM` or `SIGINT` and the bounded drain.
#[expect(
    clippy::print_stderr,
    reason = "a subscriber that cannot be installed is reported on stderr"
)]
fn serve(settings: Settings) -> ExitCode {
    runtime_then(|| async move {
        let telemetry = match telemetry(&settings.telemetry) {
            Ok(telemetry) => telemetry,
            Err(error) => {
                eprintln!("ferropix: cannot start: {}", chain(&error));
                return ExitCode::from(EXIT_CONFIG);
            }
        };
        let code = match serve::serve_settings(settings, Arc::clone(&telemetry)).await {
            Ok(()) => {
                tracing::info!("ferropix stopped");
                ExitCode::SUCCESS
            }
            Err(error) => {
                tracing::error!(error = %chain(&error), "cannot serve");
                ExitCode::FAILURE
            }
        };
        shut_down(&telemetry);
        code
    })
}

/// Runs a `db` job: the subscriber installed, then the job against the
/// store, its outcome one line on standard output.
///
/// `db migrate` applies the pending migrations, checks the sealing key and
/// applies the domain seed; `db reseal` rewrites every identifier column
/// under the configured key, reading the old one from `--old-key-file`.
#[expect(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "a db job reports its one answer on standard output and a refused key on stderr"
)]
fn db(settings: Settings, command: &DbCommand) -> ExitCode {
    let old_key = match command {
        DbCommand::Migrate => None,
        DbCommand::Reseal { old_key_file } => match read_old_key(old_key_file) {
            Ok(key) => Some(key),
            Err(error) => {
                eprintln!("ferropix: the old key is refused: {}", chain(&error));
                return ExitCode::from(EXIT_CONFIG);
            }
        },
    };
    runtime_then(|| async move {
        let telemetry = match telemetry(&settings.telemetry) {
            Ok(telemetry) => telemetry,
            Err(error) => {
                eprintln!("ferropix: cannot start: {}", chain(&error));
                return ExitCode::from(EXIT_CONFIG);
            }
        };
        let outcome = match old_key {
            None => match store::Store::start(&settings.db, &settings.domains, store::Job::Migrate)
                .await
            {
                Ok(store) => store::pseudonym::seed(&store, &settings.pseudonym_domains)
                    .await
                    .map(|_inserted| store),
                Err(error) => Err(error),
            }
            .map(|store| {
                (
                    store,
                    String::from("the migrations and the domain seed are applied"),
                )
            }),
            Some(old) => reseal(&settings, &old).await,
        };
        let code = match outcome {
            Ok((store, line)) => {
                store.close().await;
                println!("ferropix: {line}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("ferropix: the db job failed: {}", chain(&error));
                ExitCode::FAILURE
            }
        };
        shut_down(&telemetry);
        code
    })
}

/// Reads the old sealing key of `db reseal` from `path`.
fn read_old_key(path: &std::path::Path) -> Result<store::seal::SealingKey, config::error::Error> {
    let text = ferrohealth_secret::file::read_secret_file("old_key", path)?;
    store::seal::SealingKey::from_base64(secrecy::ExposeSecret::expose_secret(&text)).map_err(
        |store::seal::KeyFormatError| config::error::Error::Value {
            key: String::from("old_key_file"),
            rule: "the file holds the base64 of exactly 32 bytes",
        },
    )
}

/// Connects to the store and rewrites every identifier column from `old` to
/// the configured key.
async fn reseal(
    settings: &Settings,
    old: &store::seal::SealingKey,
) -> Result<(store::Store, String), store::StoreError> {
    let old = store::seal::Sealer::new(old)?;
    let store = store::Store::connect(&settings.db).await?;
    let report = store::reseal::run(&store, &old, store::reseal::BATCH).await?;
    let line = format!(
        "{} record(s) resealed from key {} to key {}",
        report.records, report.from, report.to
    );
    Ok((store, line))
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
