// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The BALP audit trail of the Manager and the Supplier, through `ihe-atna`.
//!
//! Every record waits in the ITI-20 store-and-forward spool `ihe-atna`
//! provides (ITI TF-2 §3.20.4.1.1: one `0600` file per record in a `0700`
//! directory) and is sent from there to the `[audit]` repository over the
//! ATX: FHIR Feed (BALP 1.1.4 §1:52.1.1.1). A transaction takes its record's
//! place in the spool before it runs ([`Audit::reserve`]), so a spool that
//! is full, unwritable or stalled refuses the transaction with `503` before
//! it changes anything, as FerroFED's does. [`records`] writes the
//! `AuditEvent` each profile prescribes. The spool's bounds and the place a
//! record reserves are our own design: no specification governs them.

pub mod records;
pub mod reserve;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ferrohealth_health::registry::{Check, HealthIndicator, Outcome};
use ferropix_operator_api::health::{self, Upstream, UpstreamKind};
use ihe_atna::atna::feed::{FeedAddressError, FeedRepository};
use ihe_atna::atna::forwarder::Forwarder;
use ihe_atna::atna::spool::{Bounds, Content, Spool, SpoolError};
use ihe_atna::balp::{Exchange, NetworkAddress, Observer, RecordError, SOURCE_APPLICATION_SERVER};
use secrecy::SecretSlice;

use crate::config::section::audit::AuditSettings;

/// The place one record reserves before its transaction runs: room for the
/// largest record a transaction writes, the raw request of a search
/// included (no specification governs the size: our own design).
pub const RESERVED_BYTES: u64 = 16 * 1024;

/// The share of either bound under which the spool's headroom is reported
/// down, so readiness warns before transactions are refused (our own
/// design).
pub const HEADROOM_PERCENT: u64 = 10;

/// Why the audit trail could not start.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OpenError {
    /// The spool directory cannot serve: open to other users, unwritable,
    /// holding a file the sender did not write, or no directory.
    #[error("the audit spool cannot be opened")]
    Spool(#[source] SpoolError),
    /// The repository's FHIR base is refused by the FHIR Feed.
    #[error("audit.url is no FHIR base the FHIR Feed can post to")]
    Repository(#[source] FeedAddressError),
    /// The HTTP client of the FHIR Feed could not be built.
    #[error("the HTTP client of the FHIR Feed could not be built")]
    Client(#[source] reqwest::Error),
}

/// Why a record could not be stored, which fails its transaction closed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Unrecorded {
    /// The spool has no room, could not write in time, or could not write.
    #[error("the audit spool could not store the record")]
    Spool(#[source] SpoolError),
    /// The record could not be written as JSON.
    #[error("the audit record could not be written")]
    Record(#[source] RecordError),
}

/// The audit trail: the spool, and the forwarder that drains it when a
/// repository is configured.
#[derive(Debug, Clone)]
pub struct Audit {
    /// The spool every record waits in.
    spool: Spool,
    /// The forwarder to the repository, when `[audit] url` names one.
    forwarder: Option<Arc<Forwarder>>,
    /// The repository's `AuditEvent` endpoint, without userinfo or query.
    endpoint: Option<String>,
    /// The recording system.
    observer: Arc<Observer>,
    /// The spool directory.
    directory: PathBuf,
    /// The spool's bounds.
    bounds: Bounds,
}

impl Audit {
    /// Opens the spool `settings` names and, when it names a repository,
    /// the FHIR Feed to it, recording as the server reached at
    /// `public_base_url`.
    ///
    /// Without a repository the records are kept in the spool until one is
    /// configured, and a full spool refuses every transaction (our own
    /// design: a record is never dropped).
    ///
    /// # Errors
    ///
    /// Returns [`OpenError::Spool`] for a spool directory that is open to
    /// its group or other users, unwritable, or holds a foreign file,
    /// [`OpenError::Repository`] for a base the FHIR Feed refuses, and
    /// [`OpenError::Client`] when the HTTP client cannot be built.
    pub fn open(settings: &AuditSettings, public_base_url: &url::Url) -> Result<Self, OpenError> {
        let bounds = Bounds {
            max_messages: settings.spool_max_events,
            max_bytes: settings.spool_max_bytes,
            write_timeout: settings.spool_write_timeout,
        };
        let spool = Spool::open_for(&settings.spool_dir, bounds, Content::AuditEvents)
            .map_err(OpenError::Spool)?;
        let (forwarder, endpoint) = if let Some(base) = &settings.url {
            let feed = feed(base, settings.timeout)?;
            let endpoint = feed.endpoint().to_string();
            (
                Some(Forwarder::fhir_feed(
                    spool.clone(),
                    feed,
                    settings.retry_max,
                )),
                Some(endpoint),
            )
        } else {
            tracing::warn!(
                "audit.url is not set: the audit records are kept in the spool until a repository is configured"
            );
            (None, None)
        };
        Ok(Self {
            spool,
            forwarder,
            endpoint,
            observer: Arc::new(observer(public_base_url)),
            directory: settings.spool_dir.clone(),
            bounds,
        })
    }

    /// Starts draining the spool to the repository, when one is configured.
    ///
    /// The task runs until it is aborted; a record in flight then stays in
    /// the spool and is sent again by the next run.
    #[must_use]
    pub fn drain(&self) -> Option<tokio::task::JoinHandle<()>> {
        self.forwarder
            .as_ref()
            .map(|forwarder| tokio::spawn(Arc::clone(forwarder).run()))
    }

    /// Takes the place of one record before its transaction runs.
    ///
    /// # Errors
    ///
    /// Returns [`Unrecorded::Spool`] when the spool has no room.
    pub fn reserve(&self) -> Result<reserve::Place, Unrecorded> {
        self.spool
            .reserve(RESERVED_BYTES)
            .map(|reservation| reserve::Place::new(reservation, Arc::clone(&self.observer)))
            .map_err(Unrecorded::Spool)
    }

    /// Stores the records of `exchanges`, each in a place of its own.
    ///
    /// # Errors
    ///
    /// Returns [`Unrecorded`] when a record cannot be written or stored.
    pub async fn record(&self, exchanges: Vec<Exchange>) -> Result<(), Unrecorded> {
        for exchange in exchanges {
            self.reserve()?.record(&exchange).await?;
        }
        Ok(())
    }

    /// Returns the directory the records wait in.
    #[must_use]
    pub fn directory(&self) -> &std::path::Path {
        &self.directory
    }

    /// Returns the repository as the dependency report names it, or `None`
    /// when none is configured.
    #[must_use]
    pub fn upstream(&self) -> Option<Upstream> {
        let forwarder = self.forwarder.as_ref()?;
        let status = forwarder.status();
        let last = (status.delivered > 0 || status.retries > 0).then(|| {
            if status.reachable {
                health::Outcome::up()
            } else {
                health::Outcome::down(format!(
                    "the last delivery failed; {} record(s) wait in the spool",
                    status.depth.waiting()
                ))
            }
        });
        Some(Upstream {
            kind: UpstreamKind::AuditRepository,
            endpoint: self.endpoint.clone().unwrap_or_default(),
            last,
            at: None,
        })
    }

    /// Returns the spool's headroom as the readiness indicator reports it.
    #[must_use]
    pub fn headroom(&self) -> Outcome {
        let depth = self.spool.depth();
        let messages = u64::try_from(depth.messages).unwrap_or(u64::MAX);
        let max_messages = u64::try_from(self.bounds.max_messages).unwrap_or(u64::MAX);
        let tight = |held: u64, bound: u64| {
            bound.saturating_sub(held).saturating_mul(100) < bound.saturating_mul(HEADROOM_PERCENT)
        };
        if tight(messages, max_messages) || tight(depth.bytes, self.bounds.max_bytes) {
            Outcome::down(format!(
                "the audit spool holds {} of {} records and {} of {} bytes ({} quarantined)",
                depth.messages,
                self.bounds.max_messages,
                depth.bytes,
                self.bounds.max_bytes,
                depth.quarantined
            ))
        } else {
            Outcome::up()
        }
    }
}

/// The readiness indicator of the spool's headroom.
#[derive(Debug, Clone)]
pub struct Headroom(pub Audit);

impl HealthIndicator for Headroom {
    fn name(&self) -> &'static str {
        "audit_spool"
    }

    fn check(&self) -> Check<'_> {
        let state = self.0.headroom();
        Box::pin(async move { state })
    }
}

/// Returns the FHIR Feed to `base`, plain `http` only where the
/// configuration admitted it (the development profile).
fn feed(base: &url::Url, timeout: Duration) -> Result<FeedRepository, OpenError> {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(OpenError::Client)?;
    if base.scheme() == "http" {
        FeedRepository::cleartext(base.clone(), http, timeout)
    } else {
        FeedRepository::new(base.clone(), http, timeout)
    }
    .map_err(OpenError::Repository)
}

/// Returns the recording system: FerroPIX at `public_base_url`, an
/// application server (FHIR R4 `security-source-type` `4`).
fn observer(public_base_url: &url::Url) -> Observer {
    let host = public_base_url.host_str().unwrap_or("ferropix");
    Observer {
        source_id: host.to_owned(),
        site: None,
        host: NetworkAddress::host(host),
        source_types: vec![SOURCE_APPLICATION_SERVER],
    }
}

/// Returns the `AuditEvent` of `exchange` as `observer` records it.
fn written(exchange: &Exchange, observer: &Observer) -> Result<SecretSlice<u8>, Unrecorded> {
    Ok(exchange
        .audit_event(observer)
        .map_err(Unrecorded::Record)?
        .into_bytes())
}
