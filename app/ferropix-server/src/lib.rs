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
pub mod mount;
pub mod operator;
pub mod outbox;
pub mod outcome;
pub mod pdqm;
pub mod problem;
pub mod serve;
pub mod store;
pub mod usage_report;
pub mod v2;
pub mod xcpd;

use std::fmt::Write as _;
use std::process::ExitCode;
use std::sync::Arc;

use ferrohealth_config::loader::Loaded;
use ferrohealth_server::cli::{Common, ConfigCommand};
use ferrohealth_server::healthcheck::Outcome;
use ferrohealth_server::run::{EXIT_CONFIG, Failure, Run, chain};
use ferrohealth_server::section::ServerSection;
use ferrohealth_server::telemetry::{Installed, TelemetryBuildError};
use ferrohealth_telemetry::section::TelemetrySection;
use ferrohealth_telemetry::telemetry::Service;

use crate::cli::{Cli, Command, DbCommand, MatchingCommand};
use crate::config::Config;
use crate::config::settings::Settings;

/// The scope of the meter the request log records
/// `http.server.request.duration` in.
pub const METER: &str = "ferropix_server";

/// The run path of the `ferropix` binary, which names it in every line it
/// writes.
pub const RUN: Run = Run::new("ferropix");

/// Runs the job `cli` names on the runtime of [`RUN`] and returns its exit
/// code.
///
/// # Errors
///
/// Returns the [`Failure`] of a configuration that does not resolve (with
/// [`EXIT_CONFIG`], or exit 1 for `healthcheck`) and of a job that fails.
pub async fn dispatch(cli: Cli) -> Result<ExitCode, Failure> {
    let loaded = Config::load(&cli.config).and_then(|loaded| {
        let mut settings = loaded.settings().resolve()?;
        settings.provenance = loaded.provenance().clone();
        Ok((loaded, settings))
    });
    let (loaded, settings) = loaded.map_err(|error| match &cli.command {
        Command::Common(common) => common.refused(error),
        Command::Db { .. } | Command::Matching { .. } => Failure::refused_configuration(error),
    })?;
    match cli.command {
        Command::Common(Common::Config {
            command: ConfigCommand::Check,
        }) => config_check(&loaded, &settings),
        Command::Common(Common::Healthcheck) => Ok(healthcheck(settings).await),
        Command::Common(Common::Serve) => Box::pin(serve(settings)).await,
        Command::Db { command } => db(settings, &command).await,
        Command::Matching {
            command: MatchingCommand::Estimate,
        } => estimate(settings).await,
    }
}

/// Runs `matching estimate`: the `u` of every field enough records state,
/// printed as the `[matching.fields]` tables that would set it, each beside
/// the `m` the configuration holds.
#[expect(
    clippy::print_stdout,
    reason = "the job prints its tables on standard output"
)]
async fn estimate(settings: Settings) -> Result<ExitCode, Failure> {
    let installed = telemetry(&settings.telemetry).map_err(Failure::refused_configuration)?;
    let outcome = async {
        let store = store::Store::connect(&settings.db).await?;
        store::key::verify(&store).await?;
        let estimates = store::estimate::estimate(&store).await;
        store.close().await;
        estimates
    }
    .await;
    installed.shutdown();
    let estimates =
        outcome.map_err(|error| Failure::from(error).with_context("the estimate failed"))?;
    println!("{}", estimate_tables(&settings.matching, &estimates));
    Ok(ExitCode::SUCCESS)
}

/// Installs the telemetry `section` asks for, described as the `ferropix`
/// service: its console, its span export over `[telemetry.otlp.tls]` and
/// the panic hook.
///
/// With `[telemetry.otlp]` the exporters speak gRPC, so this runs inside the
/// Tokio runtime that carries the export.
///
/// # Errors
/// Returns every [`TelemetryBuildError`] of `ferrohealth-server`'s
/// installer.
pub fn telemetry(section: &TelemetrySection) -> Result<Installed, TelemetryBuildError> {
    ferrohealth_server::telemetry::install(
        section,
        Service::new("ferropix", env!("CARGO_PKG_VERSION")),
    )
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
    reason = "the job's report goes to standard output"
)]
fn config_check(loaded: &Loaded<Config>, settings: &Settings) -> Result<ExitCode, Failure> {
    ferrohealth_server::run::config_check(loaded)?;
    println!(
        "ferropix: the configuration is accepted ({} profile, {} seeded domain(s))",
        match settings.profile {
            config::Profile::Production => "production",
            config::Profile::Development => "development",
        },
        settings.domains.len()
    );
    Ok(ExitCode::SUCCESS)
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
        Some(tls) => match ferrohealth_server::tls::HealthcheckTls::read_of(
            tls,
            ferrohealth_server::tls::TlsKeys::SERVER,
        ) {
            Ok(pinned) => check_tls(address, base, HEALTHCHECK_TIMEOUT, &pinned).await,
            Err(error) => Outcome::Failed(format!("the TLS files do not read: {error}")),
        },
    }
}

/// Runs `serve`: the subscriber installed, the listener bound, and the
/// routes served until `SIGTERM` or `SIGINT` and the bounded drain.
async fn serve(settings: Settings) -> Result<ExitCode, Failure> {
    let installed = telemetry(&settings.telemetry).map_err(Failure::refused_configuration)?;
    let code = match Box::pin(serve::serve_settings(
        settings,
        Arc::clone(installed.telemetry()),
    ))
    .await
    {
        Ok(()) => {
            tracing::info!("ferropix stopped");
            ExitCode::SUCCESS
        }
        Err(error) => {
            tracing::error!(error = %chain(&error), "cannot serve");
            ExitCode::FAILURE
        }
    };
    installed.shutdown();
    Ok(code)
}

/// Runs a `db` job: the subscriber installed, then the job against the
/// store, its outcome one line on standard output.
///
/// `db migrate` applies the pending migrations, checks the sealing key and
/// applies the domain seed; `db reseal` rewrites every identifier column
/// under the configured key, reading the old one from `--old-key-file`.
#[expect(
    clippy::print_stdout,
    reason = "a db job reports its one answer on standard output"
)]
async fn db(settings: Settings, command: &DbCommand) -> Result<ExitCode, Failure> {
    let old_key = match command {
        DbCommand::Migrate => None,
        DbCommand::Reseal { old_key_file } => {
            Some(read_old_key(old_key_file).map_err(|error| {
                Failure::from(error)
                    .with_context("the old key is refused")
                    .with_code(EXIT_CONFIG)
            })?)
        }
    };
    let installed = telemetry(&settings.telemetry).map_err(Failure::refused_configuration)?;
    let outcome = match old_key {
        None => {
            match store::Store::start(&settings.db, &settings.domains, store::Job::Migrate).await {
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
            })
        }
        Some(old) => reseal(&settings, &old).await,
    };
    let answer = match outcome {
        Ok((store, line)) => {
            store.close().await;
            println!("ferropix: {line}");
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => Err(Failure::from(error).with_context("the db job failed")),
    };
    installed.shutdown();
    answer
}

/// Reads the old sealing key of `db reseal` from `path`.
fn read_old_key(
    path: &std::path::Path,
) -> Result<config::section::db::SealingKey, config::error::Error> {
    let text = ferrohealth_secret::file::read_secret_file("old_key", path)?;
    config::section::db::SealingKey::from_base64(secrecy::ExposeSecret::expose_secret(&text))
        .map_err(
            |config::section::db::KeyFormatError| config::error::Error::Value {
                key: String::from("old_key_file"),
                rule: "the file holds the base64 of exactly 32 bytes",
            },
        )
}

/// Connects to the store and rewrites every identifier column from `old` to
/// the configured key.
async fn reseal(
    settings: &Settings,
    old: &config::section::db::SealingKey,
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
