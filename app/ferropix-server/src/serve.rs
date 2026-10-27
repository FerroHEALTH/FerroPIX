// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `serve` run path: the store, the outbox senders, the MLLP listener
//! and the HTTP server started from one [`Settings`], and the bounded drain
//! when the process is asked to stop.

use crate::config::settings::Settings;
use crate::{
    METER, audit, auth, config, feed, fhir, hl7v2, http, operator, outbox, store, usage_report,
    xcpd,
};
use ferrohealth_health::lifecycle::Lifecycle;
use ferrohealth_health::registry::Registry;
use ferrohealth_server::health::Health;
use ferrohealth_server::section::ServerSection;
use ferrohealth_telemetry::request_log::RequestLog;
use ferrohealth_telemetry::section::TelemetrySection;
use ferrohealth_telemetry::telemetry::Telemetry;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;

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
    /// The listener could not be bound.
    #[error("the listener could not be bound")]
    Io(#[source] std::io::Error),
    /// The listener's TLS could not be set up, or serving failed.
    #[error("the listener could not serve")]
    Serve(#[source] ferrohealth_server::serve::ServeError),
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
/// built, [`ServeError::Pmir`] when the ITI-93 sender's cannot,
/// [`ServeError::Io`] when the listener cannot be bound, and
/// [`ServeError::Serve`] when its TLS cannot be set up or serving fails.
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
        public_base_url,
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
    let facts = usage_report::Facts::new(profile, services, domains.len());
    let smart = Arc::new(
        auth::Smart::discover(auth, &domains)
            .await
            .map_err(ServeError::Auth)?,
    );
    let mounts = fhir::Mounts::of(&pixm, &pdqm, &pmir, &operator);
    let audit = audit::Audit::open(&audit, &public_base_url).map_err(ServeError::Audit)?;
    let draining = audit.drain();
    let listening = hl7v2::Listening::default();
    let health = registry(&store, &audit, hl7v2.as_ref().map(|_| &listening));
    let lifecycle = Lifecycle::default();
    let at = fhir::Address {
        base: server.base_path.as_str(),
        public_base_url: &public_base_url,
    };
    let guarded = fhir::routes(at, mounts, &smart, &health, &lifecycle, &audit, &backend);
    let (guarded, mut channels) = with_xcpd(guarded, xcpd, &store, &matching, at, &audit, &wake)?;
    channels.pmir = pmir_channel(&pmir)?;
    channels.hl7v2 = mllp_channel(hl7v2.as_ref(), &audit);
    let window = Arc::new(usage_report::window(server.base_path.as_str()));
    let listener = bind(&server).await?;
    let reporting = usage_report::start(
        &report_settings,
        usage_report::Pix {
            facts,
            health: health.clone(),
            lifecycle: lifecycle.clone(),
        },
        Arc::clone(&window),
        started,
        ferrohealth_usage_report::section::ci_of(|name| std::env::var(name).ok()),
    )
    .await;
    let (signal, stopped) = stop_signal(lifecycle.clone());
    let sender = outbox::start(store.clone(), channels, retention, wake, stopped.clone());
    let mllp = match &hl7v2 {
        Some(settings) => Some(
            start_mllp(
                settings,
                hl7v2::Face::new(
                    &store,
                    &matching,
                    pdqm.deprecated,
                    &public_base_url,
                    &audit,
                    settings,
                )
                .with_usage(Arc::clone(&window)),
                server.shutdown_timeout(),
                listening,
                stopped_by(stopped.clone()),
            )
            .await?,
        ),
        None => None,
    };
    let log = request_log(&telemetry_section, &telemetry, window);
    let health = Health::new(health, lifecycle.clone());
    let app = http::router(&server, health, Some(guarded), log, Some(telemetry));
    let served = serve_http(listener, app, &server, lifecycle, stopped).await;
    signal.abort();
    wound_down(
        mllp,
        sender,
        server.shutdown_timeout(),
        [draining, reporting],
    )
    .await;
    store.close().await;
    served
}

/// Serves `app` on `listener` as `server` configures it, over TLS when
/// `[server.tls]` is set, until `stopped` is raised, then drains within
/// `server.drain_delay_ms` and `server.shutdown_timeout_ms`.
///
/// # Errors
/// Returns [`ServeError::Serve`] when the TLS files cannot be read or the
/// listener fails.
async fn serve_http(
    listener: TcpListener,
    app: axum::Router,
    server: &ServerSection,
    lifecycle: Lifecycle,
    stopped: tokio::sync::watch::Receiver<bool>,
) -> Result<(), ServeError> {
    ferrohealth_server::serve::run(listener, app, server, lifecycle, stopped_by(stopped))
        .await
        .map(|_drained| ())
        .map_err(ServeError::Serve)
}

/// Returns the request log of the HTTP listener: the query values `section`
/// allows, the duration histogram of `telemetry`, and the usage report's
/// `window` fed with every finished request.
fn request_log(
    section: &TelemetrySection,
    telemetry: &Telemetry,
    window: Arc<ferrohealth_usage_report::window::Window>,
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
    at: fhir::Address<'_>,
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
        at.public_base_url,
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
    let public = &settings.public_base_url;
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
) -> Registry {
    let health = Registry::default()
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
async fn bind(server: &ServerSection) -> Result<TcpListener, ServeError> {
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
fn services(settings: &Settings) -> Vec<usage_report::Service> {
    [
        (settings.hl7v2.is_some(), usage_report::Service::Hl7v2),
        (settings.operator.enabled, usage_report::Service::Operator),
        (settings.pdqm.enabled, usage_report::Service::Pdqm),
        (settings.pixm.enabled, usage_report::Service::Pixm),
        (settings.pmir.enabled, usage_report::Service::Pmir),
    ]
    .into_iter()
    .filter_map(|(on, service)| on.then_some(service))
    .collect()
}

/// Waits for `SIGTERM` or `SIGINT` in a task of its own and returns the
/// task and the flag it raises, which every listener drains on; `lifecycle`
/// turns to draining first, so readiness answers `503` while the requests in
/// flight finish.
fn stop_signal(
    lifecycle: Lifecycle,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::watch::Receiver<bool>,
) {
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let signal = tokio::spawn(async move {
        ferrohealth_server::serve::shutdown_signal().await;
        lifecycle.drain();
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
