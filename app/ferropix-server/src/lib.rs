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
pub mod telemetry;
pub mod usage_report;
pub mod xcpd;

use std::error::Error;
use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use tokio::net::TcpListener;

use crate::cli::{Cli, Command, ConfigCommand, DbCommand, MatchingCommand};
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
    if let Err(error) = telemetry::init(&settings.telemetry, std::io::stdout().is_terminal(), None)
    {
        eprintln!("ferropix: cannot start: {}", chain(&error));
        return ExitCode::from(EXIT_CONFIG);
    }
    runtime_then(|| async move {
        let outcome = async {
            let store = store::Store::connect(&settings.db).await?;
            store::key::verify(&store).await?;
            let estimates = store::estimate::estimate(&store).await;
            store.close().await;
            estimates
        }
        .await;
        match outcome {
            Ok(estimates) => {
                println!("{}", estimate_tables(&settings.matching, &estimates));
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("ferropix: the estimate failed: {}", chain(&error));
                ExitCode::FAILURE
            }
        }
    })
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
    runtime_then(|| async move {
        let traces = match settings
            .telemetry
            .otlp_endpoint
            .as_ref()
            .map(telemetry::Traces::new)
        {
            None => None,
            Some(Ok(traces)) => Some(traces),
            Some(Err(error)) => {
                eprintln!("ferropix: cannot start: {}", chain(&error));
                return ExitCode::from(EXIT_CONFIG);
            }
        };
        let exporting = traces.as_ref().map(telemetry::Traces::tracer);
        if let Err(error) = telemetry::init(
            &settings.telemetry,
            std::io::stdout().is_terminal(),
            exporting,
        ) {
            eprintln!("ferropix: cannot start: {}", chain(&error));
            return ExitCode::from(EXIT_CONFIG);
        }
        let code = match serve_settings(settings).await {
            Ok(()) => {
                tracing::info!("ferropix stopped");
                ExitCode::SUCCESS
            }
            Err(error) => {
                tracing::error!(error = %chain(&error), "cannot serve");
                ExitCode::FAILURE
            }
        };
        if let Some(traces) = traces
            && let Err(error) = traces.shutdown()
        {
            tracing::warn!(error = %chain(&error), "the span export did not flush");
        }
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
    if let Err(error) = telemetry::init(&settings.telemetry, std::io::stdout().is_terminal(), None)
    {
        eprintln!("ferropix: cannot start: {}", chain(&error));
        return ExitCode::from(EXIT_CONFIG);
    }
    runtime_then(|| async move {
        let outcome = match old_key {
            None => store::Store::start(&settings.db, &settings.domains, store::Job::Migrate)
                .await
                .map(|store| {
                    (
                        store,
                        String::from("the migrations and the domain seed are applied"),
                    )
                }),
            Some(old) => reseal(&settings, &old).await,
        };
        match outcome {
            Ok((store, line)) => {
                store.close().await;
                println!("ferropix: {line}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("ferropix: the db job failed: {}", chain(&error));
                ExitCode::FAILURE
            }
        }
    })
}

/// Reads the old sealing key of `db reseal` from `path`.
fn read_old_key(path: &std::path::Path) -> Result<store::seal::SealingKey, config::error::Error> {
    let text = config::secret::read_file("old_key_file", path)?;
    store::seal::SealingKey::from_base64(text.expose()).map_err(|store::seal::KeyFormatError| {
        config::error::Error::Value {
            key: String::from("old_key_file"),
            rule: "the file holds the base64 of exactly 32 bytes",
        }
    })
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
/// built, and [`ServeError::Io`] for the I/O error from binding, accepting or
/// serving.
pub async fn serve_settings(settings: Settings) -> Result<(), ServeError> {
    let started = Instant::now();
    let store = store::Store::start(&settings.db, &settings.domains, store::Job::Serve)
        .await
        .map_err(ServeError::Store)?;
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
    let wake = outbox::Wake::default();
    let (guarded, channels) = with_xcpd(guarded, xcpd, &store, &matching, &server, &audit, &wake)?;
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
    let served = http::serve_until(
        listener,
        http::router(&server, &health, Some(guarded)).layer(axum::Extension(window)),
        server.drain_timeout,
        stopped_by(stopped),
    )
    .await
    .map_err(ServeError::Io);
    signal.abort();
    wound_down(mllp, sender, server.drain_timeout, [draining, reporting]).await;
    store.close().await;
    served
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
    };
    Ok((guarded.merge(xcpd::routes(gateway)), channels))
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
            origins: settings.origins.clone(),
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
/// headroom and, when the HL7 v2 face runs, its listener.
fn registry(
    store: &store::Store,
    audit: &audit::Audit,
    listening: Option<&hl7v2::Listening>,
) -> health::Registry {
    let health = health::Registry::default()
        .and(Arc::new(store::indicator::Database::new(store.clone())))
        .and(Arc::new(audit::Headroom(audit.clone())));
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
