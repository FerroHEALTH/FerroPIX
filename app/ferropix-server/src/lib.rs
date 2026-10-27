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
pub mod health;
pub mod healthcheck;
pub mod hl7v2;
pub mod http;
pub mod matching;
pub mod operator;
pub mod outbox;
pub mod pdqm;
pub mod store;
pub mod usage_report;
pub mod xcpd;

use std::error::Error;
use std::fmt::Write as _;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use ferrohealth_config::loader::Loaded;
use ferrohealth_telemetry::request_log::RequestLog;
use ferrohealth_telemetry::section::TelemetrySection;
use ferrohealth_telemetry::telemetry::{Console, InitError, Service, Telemetry};
use tokio::net::TcpListener;

use crate::cli::{Cli, Command, ConfigCommand, DbCommand, MatchingCommand};
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
        } => config_check(&loaded, &settings),
        Command::Healthcheck => runtime_then(|| healthcheck(settings)),
        Command::Serve => serve(settings),
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
    runtime_then(|| async move {
        let telemetry = match telemetry(&settings.telemetry) {
            Ok(telemetry) => telemetry,
            Err(error) => {
                eprintln!("ferropix: cannot start: {}", chain(&error));
                return ExitCode::from(EXIT_CONFIG);
            }
        };
        let code = match serve_settings(settings, Arc::clone(&telemetry)).await {
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

/// Why `serve` could not run.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServeError {
    /// The store could not be started.
    #[error("the store could not be started")]
    Store(#[source] store::StoreError),
    /// The resource server could not start.
    #[error("the resource server could not start")]
    Auth(#[source] auth::SmartError),
    /// The audit spool or the FHIR Feed could not start.
    #[error("the audit trail could not start")]
    Audit(#[source] audit::OpenError),
    /// The XCPD Responding Gateway's HTTP client could not be built.
    #[error("the XCPD Responding Gateway could not start")]
    Xcpd(#[source] reqwest::Error),
    /// The client that delivers ITI-93 to subscribers could not be built.
    #[error("the ITI-93 sender's HTTP client could not be built")]
    Pmir(#[source] reqwest::Error),
    /// The listener could not be bound, or serving failed.
    #[error("the listener could not serve")]
    Io(#[source] std::io::Error),
}

/// Starts the store, reads the authorization servers, binds the listener
/// `settings` names and serves until the process is asked to stop.
///
/// The store passes the boot gate, applies the pending migrations unless
/// `[db] migrate = false`, checks the sealing key and applies the domain
/// seed before anything else starts; the Sources the guard admits are the
/// domains the store then holds. Once the listener is bound, the usage report
/// logs its boot line and, unless it is off, sends its start report and then
/// one a day in the background, reading the window the request log fills.
///
/// # Errors
/// Returns [`ServeError::Store`] when the store refuses to start,
/// [`ServeError::Auth`] when an authorization server cannot be read,
/// [`ServeError::Audit`] when the audit spool or its repository cannot
/// start, [`ServeError::Xcpd`] when the XCPD gateway's client cannot be
/// built, [`ServeError::Pmir`] when the ITI-93 sender's cannot, and
/// [`ServeError::Io`] for the I/O error from binding, accepting or
/// serving.
pub async fn serve_settings(
    settings: Settings,
    telemetry: Arc<Telemetry>,
) -> Result<(), ServeError> {
    let started = Instant::now();
    let wake = outbox::Wake::default();
    let store = start_store(&settings, &wake).await?;
    let domains = store::seed::stored(&store)
        .await
        .map_err(ServeError::Store)?;
    let services = services(&settings);
    let backend = backend(&store, &settings);
    let Settings {
        profile,
        server,
        auth,
        pixm,
        pdqm,
        pmir,
        hl7v2,
        xcpd,
        outbox: retention,
        operator,
        audit,
        matching,
        usage_report: report_settings,
        telemetry: telemetry_section,
        ..
    } = settings;
    let facts = usage_report::reporter::Facts::new(profile, services, domains.len());
    let smart = Arc::new(
        auth::Smart::discover(auth, &domains)
            .await
            .map_err(ServeError::Auth)?,
    );
    let mounts = fhir::Mounts::of(&pixm, &pdqm, &pmir, &operator);
    let audit = audit::Audit::open(&audit, &server.public_base_url).map_err(ServeError::Audit)?;
    let draining = audit.drain();
    let listening = hl7v2::Listening::default();
    let health = registry(&store, &audit, hl7v2.as_ref().map(|_| &listening));
    let guarded = fhir::routes(
        &server.base_path,
        &server.public_base_url,
        mounts,
        &smart,
        &health,
        &audit,
        &backend,
    );
    let (guarded, mut channels) =
        with_xcpd(guarded, xcpd, &store, &matching, &server, &audit, &wake)?;
    channels.pmir = pmir_channel(&pmir)?;
    channels.hl7v2 = mllp_channel(hl7v2.as_ref(), &audit);
    let window = Arc::new(usage_report::window::Window::new(&server.base_path));
    let listener = bind(&server).await?;
    let reporting = usage_report::reporter::start(
        &report_settings,
        usage_report::reporter::Sources {
            facts,
            health: health.clone(),
            window: Arc::clone(&window),
        },
        started,
        usage_report::ci_of(|name| std::env::var(name).ok()),
    )
    .await;
    let (signal, stopped) = stop_signal();
    let sender = outbox::start(store.clone(), channels, retention, wake, stopped.clone());
    let mllp = match &hl7v2 {
        Some(settings) => Some(
            start_mllp(
                settings,
                hl7v2::Face::new(
                    &store,
                    &matching,
                    pdqm.deprecated,
                    &server.public_base_url,
                    &audit,
                    settings,
                )
                .with_usage(Arc::clone(&window)),
                server.drain_timeout,
                listening,
                stopped_by(stopped.clone()),
            )
            .await?,
        ),
        None => None,
    };
    let log = request_log(&telemetry_section, &telemetry, window);
    let app = http::router(&server, &health, Some(guarded), log, Some(telemetry));
    let served = http::serve_until(listener, app, server.drain_timeout, stopped_by(stopped))
        .await
        .map_err(ServeError::Io);
    signal.abort();
    wound_down(mllp, sender, server.drain_timeout, [draining, reporting]).await;
    store.close().await;
    served
}

/// Returns the request log of the HTTP listener: the query values `section`
/// allows, the duration histogram of `telemetry`, and the usage report's
/// `window` fed with every finished request.
fn request_log(
    section: &TelemetrySection,
    telemetry: &Telemetry,
    window: Arc<usage_report::window::Window>,
) -> RequestLog {
    RequestLog::new(&section.logged_query_parameters)
        .with_metrics(&telemetry.meter(METER))
        .with_recorder(window)
}

/// Waits for the MLLP listener's drain and the outbox sender's message in
/// flight, then stops the background tasks `tasks`.
async fn wound_down(
    mllp: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
    sender: tokio::task::JoinHandle<()>,
    drain: std::time::Duration,
    tasks: [Option<tokio::task::JoinHandle<()>>; 2],
) {
    if let Some(mllp) = mllp {
        drained(mllp).await;
    }
    sent(sender, drain).await;
    for task in tasks.into_iter().flatten() {
        task.abort();
    }
}

/// Returns `guarded` with the XCPD Responding Gateway's route when `[xcpd]`
/// is on, and the outbox channels that deliver its answers.
fn with_xcpd(
    guarded: axum::Router,
    settings: Option<config::section::xcpd::XcpdSettings>,
    store: &store::Store,
    matching: &config::section::matching::Matching,
    server: &config::section::server::ServerSettings,
    audit: &audit::Audit,
    wake: &outbox::Wake,
) -> Result<(axum::Router, outbox::Channels), ServeError> {
    let Some(settings) = settings else {
        return Ok((guarded, outbox::Channels::default()));
    };
    let gateway = xcpd::Gateway::new(
        store,
        matching,
        settings,
        &server.public_base_url,
        audit,
        wake.clone(),
    )
    .map_err(ServeError::Xcpd)?;
    let channels = outbox::Channels {
        xcpd: Some(gateway.channel()),
        pmir: None,
        hl7v2: None,
    };
    Ok((guarded.merge(xcpd::routes(gateway)), channels))
}

/// Starts the store `settings` name for `serve`, seeds its pseudonymisation
/// domains and has it announce changes when `[pmir]` is on, waking `wake`.
///
/// # Errors
/// Returns [`ServeError::Store`] when the store refuses to start or seed.
async fn start_store(settings: &Settings, wake: &outbox::Wake) -> Result<store::Store, ServeError> {
    let store = store::Store::start(&settings.db, &settings.domains, store::Job::Serve)
        .await
        .map_err(ServeError::Store)?;
    store::pseudonym::seed(&store, &settings.pseudonym_domains)
        .await
        .map_err(ServeError::Store)?;
    Ok(with_consumers(
        with_feed(store, settings, wake),
        settings,
        wake,
    ))
}

/// Returns `store` notifying every cross-reference change as an ITI-10
/// message to each consumer it concerns, when `[[hl7v2.consumers]]` names
/// one (ITI TF-2 §3.10.4.1.1).
fn with_consumers(store: store::Store, settings: &Settings, wake: &outbox::Wake) -> store::Store {
    let Some(hl7v2) = settings.hl7v2.as_ref().filter(|h| !h.consumers.is_empty()) else {
        return store;
    };
    store.with_consumers(feed::notify::Notifying {
        manager: hl7v2.manager.clone(),
        consumers: hl7v2.consumers.clone().into(),
        authorities: hl7v2::authority::Authorities::new(&hl7v2.namespaces),
        notification: hl7v2.notification,
        wake: wake.clone(),
    })
}

/// The outbox channel of the ITI-10 notifications when `[[hl7v2.consumers]]`
/// names one: the consumers, the `[hl7v2.notification]` retry waits, the
/// listener's frame limit for an acknowledgment, and the audit trail.
fn mllp_channel(
    hl7v2: Option<&config::section::hl7v2::Hl7v2Settings>,
    audit: &audit::Audit,
) -> Option<outbox::MllpChannel> {
    let hl7v2 = hl7v2.filter(|h| !h.consumers.is_empty())?;
    Some(outbox::MllpChannel {
        consumers: hl7v2.consumers.clone().into(),
        authorities: hl7v2::authority::Authorities::new(&hl7v2.namespaces),
        frame_limit: hl7v2.frame_limit,
        backoff: outbox::Backoff {
            initial: std::time::Duration::from_millis(hl7v2.notification.initial_backoff_ms),
            max: std::time::Duration::from_millis(hl7v2.notification.max_backoff_ms),
        },
        audit: audit.clone(),
    })
}

/// Returns `store` announcing every change to a person as an ITI-93 message
/// when `[pmir]` is on (PMIR 1.6.0 §2:3.93.4.1.1).
fn with_feed(store: store::Store, settings: &Settings, wake: &outbox::Wake) -> store::Store {
    if !settings.pmir.enabled {
        return store;
    }
    let public = &settings.server.public_base_url;
    let master = public.as_str().trim_end_matches('/').to_owned();
    let registry = url::Url::parse(&format!("{master}/fhir")).unwrap_or_else(|_| public.clone());
    store.with_feed(store::subscription::FeedSettings {
        registry,
        master,
        max_attempts: settings.pmir.feed.max_attempts,
        wake: wake.clone(),
    })
}

/// The outbox channel of the ITI-93 messages to subscribers when `[pmir]`
/// is on: a client that follows no redirect, with the `[pmir.feed]` retry
/// waits.
///
/// # Errors
/// Returns [`ServeError::Pmir`] when the client cannot be built.
fn pmir_channel(
    pmir: &config::section::profiles::Pmir,
) -> Result<Option<outbox::HttpChannel>, ServeError> {
    if !pmir.enabled {
        return Ok(None);
    }
    let feed = &pmir.feed;
    // NOTE: no specification governs this: our own design; a subscriber's redirect would steer the server's posts, as D33 refuses for XCPD.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(ServeError::Pmir)?;
    Ok(Some(outbox::HttpChannel {
        client,
        backoff: outbox::Backoff {
            initial: std::time::Duration::from_millis(feed.initial_backoff_ms),
            max: std::time::Duration::from_millis(feed.max_backoff_ms),
        },
    }))
}

/// Waits for the outbox sender to finish the message in flight, at most
/// `drain`; a message it never reached stays in the outbox for the next start.
async fn sent(sender: tokio::task::JoinHandle<()>, drain: std::time::Duration) {
    match tokio::time::timeout(drain, sender).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::error!(%error, "the outbox sender's task failed"),
        Err(_) => tracing::warn!(
            drain_ms = drain.as_millis(),
            "the outbox sender did not finish within the drain; its message falls due again after its lease"
        ),
    }
}

/// Returns what the FHIR routes and the operator API read from `settings`:
/// the store, the matching settings, the deprecated PDQm search parameters,
/// where each setting came from with the HL7 v2 senders, and whether the
/// `[dev]` table is set.
fn backend(store: &store::Store, settings: &Settings) -> fhir::Backend {
    fhir::Backend {
        store: store.clone(),
        matching: settings.matching.clone(),
        deprecated: settings.pdqm.deprecated,
        configured: operator::Configured {
            provenance: settings.provenance.clone(),
            senders: settings
                .hl7v2
                .as_ref()
                .map(|h| h.senders.clone())
                .unwrap_or_default(),
        },
        development: settings.dev.is_some(),
    }
}

/// Returns the readiness registry: the database, the audit spool's
/// headroom, the outbox sender's backlog and, when the HL7 v2 face runs, its
/// listener.
fn registry(
    store: &store::Store,
    audit: &audit::Audit,
    listening: Option<&hl7v2::Listening>,
) -> health::Registry {
    let health = health::Registry::default()
        .and(Arc::new(store::indicator::Database::new(store.clone())))
        .and(Arc::new(audit::Headroom(audit.clone())))
        .and(Arc::new(outbox::Backlog::new(store.clone())));
    match listening {
        Some(listening) => health.and(Arc::new(listening.clone())),
        None => health,
    }
}

/// Binds the HTTP listener `server` names and logs the serving line.
///
/// # Errors
/// Returns [`ServeError::Io`] when the listener cannot be bound.
async fn bind(server: &config::section::server::ServerSettings) -> Result<TcpListener, ServeError> {
    let listener = TcpListener::bind(server.listen)
        .await
        .map_err(ServeError::Io)?;
    tracing::info!(
        listen = %listener.local_addr().map_err(ServeError::Io)?,
        base_path = server.base_path.as_str(),
        version = env!("CARGO_PKG_VERSION"),
        "ferropix serving"
    );
    Ok(listener)
}

/// Returns the services `settings` switch on, as the usage report names
/// them.
fn services(settings: &Settings) -> Vec<usage_report::payload::Service> {
    [
        (
            settings.hl7v2.is_some(),
            usage_report::payload::Service::Hl7v2,
        ),
        (
            settings.operator.enabled,
            usage_report::payload::Service::Operator,
        ),
        (settings.pdqm.enabled, usage_report::payload::Service::Pdqm),
        (settings.pixm.enabled, usage_report::payload::Service::Pixm),
        (settings.pmir.enabled, usage_report::payload::Service::Pmir),
    ]
    .into_iter()
    .filter_map(|(on, service)| on.then_some(service))
    .collect()
}

/// Waits for `SIGTERM` or `SIGINT` in a task of its own and returns the
/// task and the flag it raises, which every listener drains on.
fn stop_signal() -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::watch::Receiver<bool>,
) {
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let signal = tokio::spawn(async move {
        http::shutdown_signal().await;
        let _sent = stop.send(true);
    });
    (signal, stopped)
}

/// Completes once `stopped` is raised.
async fn stopped_by(mut stopped: tokio::sync::watch::Receiver<bool>) {
    let _changed = stopped.wait_for(|stop| *stop).await;
}

/// Waits for the MLLP listener's task to finish its drain, logging how it
/// ended.
async fn drained(mllp: tokio::task::JoinHandle<std::io::Result<()>>) {
    match mllp.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::error!(%error, "the MLLP listener failed"),
        Err(error) => tracing::error!(%error, "the MLLP listener's task failed"),
    }
}

/// Binds the MLLP listener `settings` names and serves `face` on it in a
/// task of its own until `shutdown` completes, draining within `drain`.
///
/// # Errors
/// Returns [`ServeError::Io`] when the listener cannot be bound.
async fn start_mllp<F>(
    settings: &config::section::hl7v2::Hl7v2Settings,
    face: hl7v2::Face,
    drain: std::time::Duration,
    listening: hl7v2::Listening,
    shutdown: F,
) -> Result<tokio::task::JoinHandle<std::io::Result<()>>, ServeError>
where
    F: Future<Output = ()> + Send + 'static,
{
    let socket = TcpListener::bind(settings.listen)
        .await
        .map_err(ServeError::Io)?;
    tracing::info!(
        listen = %socket.local_addr().map_err(ServeError::Io)?,
        "the MLLP listener is accepting"
    );
    Ok(tokio::spawn(hl7v2::serve_until(
        socket,
        Arc::new(face),
        hl7v2::Listener::of(settings, drain),
        listening,
        shutdown,
    )))
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
