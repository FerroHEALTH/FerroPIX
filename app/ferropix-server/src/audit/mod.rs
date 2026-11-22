// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The BALP audit trail of the Manager and the Supplier, through `ihe-atna`.
//!
//! The records wait in one of two ITI-20 store-and-forward stores (ITI TF-2
//! §3.20.4.1.1) and are sent from there to the `[audit]` repository over the
//! ATX: FHIR Feed (BALP 1.1.4 §1:52.1.1.1). A write's record is inserted
//! into the store's audit outbox in the write's own transaction
//! ([`Audit::enlist`], [`outbox`]), so the change and its record commit
//! together or not at all (#742). A read's record waits in the spool
//! `ihe-atna` provides (segment files in a `0700` directory): a read takes
//! its record's place before it runs ([`Audit::reserve`]), so a spool that is
//! full refuses it with `503` before it reads anything, and a record the
//! spool cannot store in time is withdrawn and the read recorded and
//! answered as refused. [`records`] writes the `AuditEvent` each profile
//! prescribes. The two stores, the spool's bounds and the place a record
//! reserves are our own design: no specification governs them.

pub mod outbox;
pub mod records;
pub mod reserve;

use std::path::PathBuf;
use std::sync::Arc;

use ferrohealth_health::registry::{Check, HealthIndicator, Outcome};
use ferrohealth_smart::client::outbound::{OutboundClient, SendError};
use ferropix_operator_api::health::{self, Upstream, UpstreamKind};
use http::Method;
use http::header::CONTENT_TYPE;
use ihe_atna::atna::feed::{
    FeedAnswer, FeedClient, FeedConfigError, FeedRepository, FeedSendError,
};
use ihe_atna::atna::forwarder::Forwarder;
use ihe_atna::atna::spool::{Bounds, Content, Spool, SpoolError};
use ihe_atna::balp::{Exchange, NetworkAddress, Observer, RecordError, SOURCE_APPLICATION_SERVER};
use secrecy::SecretSlice;
use sqlx::PgConnection;

use crate::audit::outbox::{AuditOutbox, KeepError};
use crate::config::section::audit::AuditSettings;
use crate::store::Store;

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
    /// The repository's endpoint is refused by the FHIR Feed.
    #[error("audit.endpoint is no FHIR base the FHIR Feed can post to")]
    Repository(#[source] FeedConfigError),
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
    /// The audit outbox could not take a write's record.
    #[error("the audit outbox could not keep the record")]
    Outbox(#[source] KeepError),
}

/// The audit trail: the spool and the audit outbox, and the forwarders that
/// drain them when a repository is configured.
#[derive(Debug, Clone)]
pub struct Audit {
    /// The spool every read's record waits in.
    spool: Spool,
    /// The forwarder of the spool to the repository, when `[audit.endpoint]
    /// url` names one.
    forwarder: Option<Arc<Forwarder>>,
    /// The outbound client of the repository, when one is configured.
    client: Option<OutboundClient>,
    /// The audit outbox every write's record waits in, once
    /// [`Audit::with_store`] named the store.
    outbox: Option<AuditOutbox>,
    /// The forwarder of the audit outbox, when a repository is configured.
    outbox_forwarder: Option<Arc<Forwarder<AuditOutbox>>>,
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
    /// its group or other users, unwritable, or holds a foreign file, and
    /// [`OpenError::Repository`] for an endpoint the FHIR Feed refuses.
    pub fn open(settings: &AuditSettings, public_base_url: &url::Url) -> Result<Self, OpenError> {
        let bounds = Bounds {
            max_messages: settings.spool_max_events,
            max_bytes: settings.spool_max_bytes,
            write_timeout: settings.spool_write_timeout,
        };
        let spool = Spool::open_for(&settings.spool_dir, bounds, Content::AuditEvents)
            .map_err(OpenError::Spool)?;
        let (forwarder, endpoint) = if let Some(client) = &settings.repository {
            let feed = feed(client.clone())?;
            let endpoint = feed.endpoint().to_string();
            (
                Some(Forwarder::fhir_feed(spool.clone(), feed)),
                Some(endpoint),
            )
        } else {
            tracing::warn!(
                "audit.endpoint.url is not set: the audit records are kept in the spool until a repository is configured"
            );
            (None, None)
        };
        Ok(Self {
            spool,
            forwarder,
            client: settings.repository.clone(),
            outbox: None,
            outbox_forwarder: None,
            endpoint,
            observer: Arc::new(observer(public_base_url)),
            directory: settings.spool_dir.clone(),
            bounds,
        })
    }

    /// Keeps the records of the writes in the audit outbox of `store`, and,
    /// when a repository is configured, forwards them from there.
    ///
    /// # Errors
    ///
    /// Returns [`OpenError::Repository`] for an endpoint the FHIR Feed
    /// refuses.
    pub fn with_store(mut self, store: &Store) -> Result<Self, OpenError> {
        let outbox = AuditOutbox::new(store.clone());
        self.outbox_forwarder = match &self.client {
            Some(client) => Some(Forwarder::fhir_feed_from(
                outbox.clone(),
                feed(client.clone())?,
            )),
            None => None,
        };
        self.outbox = Some(outbox);
        Ok(self)
    }

    /// Starts draining the spool and the audit outbox to the repository,
    /// when one is configured.
    ///
    /// The task runs until it is aborted; a record in flight then stays
    /// where it waits and is sent again by the next run.
    #[must_use]
    pub fn drain(&self) -> Option<tokio::task::JoinHandle<()>> {
        let spool = self.forwarder.as_ref().map(Arc::clone);
        let outbox = self.outbox_forwarder.as_ref().map(Arc::clone);
        if spool.is_none() && outbox.is_none() {
            return None;
        }
        Some(tokio::spawn(async move {
            let spool = async move {
                if let Some(forwarder) = spool {
                    forwarder.run().await;
                }
            };
            let outbox = async move {
                if let Some(forwarder) = outbox {
                    forwarder.run().await;
                }
            };
            tokio::join!(spool, outbox);
        }))
    }

    /// Inserts the records of `exchanges` into the audit outbox through
    /// `connection`, sealed under the store's key: in the write's
    /// transaction, before COMMIT, so the change and its records commit
    /// together, or, for a write refused before it changed anything, in a
    /// statement of its own.
    ///
    /// # Errors
    ///
    /// Returns [`Unrecorded::Record`] when a record cannot be written and
    /// [`Unrecorded::Outbox`] when the outbox cannot take it, or when no
    /// store was named.
    pub async fn enlist(
        &self,
        connection: &mut PgConnection,
        exchanges: &[Exchange],
    ) -> Result<(), Unrecorded> {
        let Some(outbox) = &self.outbox else {
            return Err(Unrecorded::Outbox(KeepError::Unattached));
        };
        let mut records = Vec::with_capacity(exchanges.len());
        for exchange in exchanges {
            records.push(written(exchange, &self.observer)?);
        }
        outbox::insert(connection, outbox.sealer(), &records)
            .await
            .map_err(Unrecorded::Outbox)
    }

    /// Keeps the records of `exchanges`, of a write refused before it
    /// changed anything, in the audit outbox in a statement of their own.
    ///
    /// # Errors
    ///
    /// As [`Audit::enlist`], and [`Unrecorded::Outbox`] when the store lends
    /// no connection.
    pub async fn keep(&self, exchanges: &[Exchange]) -> Result<(), Unrecorded> {
        let Some(outbox) = &self.outbox else {
            return Err(Unrecorded::Outbox(KeepError::Unattached));
        };
        let mut connection = outbox
            .connection()
            .await
            .map_err(|error| Unrecorded::Outbox(KeepError::Store(error)))?;
        self.enlist(&mut connection, exchanges).await?;
        outbox.committed();
        Ok(())
    }

    /// Wakes the audit outbox's forwarder once a write committed records.
    pub fn committed(&self) {
        if let Some(outbox) = &self.outbox {
            outbox.committed();
        }
    }

    /// Returns the audit outbox, once [`Audit::with_store`] named the store.
    #[must_use]
    pub fn outbox(&self) -> Option<&AuditOutbox> {
        self.outbox.as_ref()
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

    /// Stores the records of a read's `exchanges` in the spool as one
    /// batch, or the records of the read refused when they are late.
    ///
    /// # Errors
    ///
    /// Returns [`Unrecorded`] as [`reserve::Place::record`] does.
    pub async fn record(&self, exchanges: &[Exchange]) -> Result<(), Unrecorded> {
        if exchanges.is_empty() {
            return Ok(());
        }
        self.reserve()?.record(exchanges).await
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
        let spool = self.forwarder.as_ref()?.status();
        let outbox = self
            .outbox_forwarder
            .as_ref()
            .map(|forwarder| forwarder.status());
        let tried =
            |status: &ihe_atna::atna::forwarder::Status| status.delivered > 0 || status.retries > 0;
        let unreachable =
            |status: &ihe_atna::atna::forwarder::Status| tried(status) && !status.reachable;
        let last = (tried(&spool) || outbox.as_ref().is_some_and(tried)).then(|| {
            if unreachable(&spool) || outbox.as_ref().is_some_and(unreachable) {
                health::Outcome::down(format!(
                    "the last delivery failed; {} record(s) wait in the spool and {} in the audit outbox",
                    spool.depth.waiting(),
                    outbox.as_ref().map_or(0, |status| status.depth.waiting())
                ))
            } else {
                health::Outcome::up()
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

/// The readiness indicator of the audit outbox's quarantine: down while it
/// holds a record the repository refused, so an operator looks at it (no
/// specification governs this: our own design).
#[derive(Debug, Clone)]
pub struct Quarantine(pub Audit);

impl HealthIndicator for Quarantine {
    fn name(&self) -> &'static str {
        "audit_outbox"
    }

    fn check(&self) -> Check<'_> {
        let depth = self
            .0
            .outbox
            .as_ref()
            .map(ihe_atna::atna::forwarder::Outbox::depth)
            .unwrap_or_default();
        let state = if depth.quarantined > 0 {
            Outcome::down(format!(
                "the audit outbox holds {} record(s) the repository refused, in pix.audit_quarantine, and {} waiting",
                depth.quarantined,
                depth.waiting()
            ))
        } else {
            Outcome::up()
        };
        Box::pin(async move { state })
    }
}

/// The FHIR Feed's client: the server's own outbound client of
/// `[audit.repository.endpoint]`, with its TLS, timeouts, credential and
/// retries (no specification governs the seam: `ihe-atna`'s design).
struct Feed(OutboundClient);

#[async_trait::async_trait]
impl FeedClient for Feed {
    fn authorizes(&self) -> bool {
        self.0.authorization().is_some()
    }

    async fn post(
        &self,
        url: &url::Url,
        content_type: &'static str,
        body: &[u8],
    ) -> Result<FeedAnswer, FeedSendError> {
        let answer = self
            .0
            .send(&Method::POST, url, |request| {
                request
                    .header(CONTENT_TYPE, content_type)
                    .body(body.to_vec())
            })
            .await
            .map_err(|error| match error {
                SendError::Transport(error) => {
                    FeedSendError::Transport(Box::new(error.without_url()))
                }
                other => FeedSendError::Credentials(Box::new(other)),
            })?;
        let status = answer.status();
        let body = answer
            .bytes()
            .await
            .map_err(|error| FeedSendError::Transport(Box::new(error.without_url())))?;
        Ok(FeedAnswer::new(status, body.to_vec()))
    }
}

/// Returns the FHIR Feed through `client`, plain `http` only where the
/// configuration admitted it (the development profile), waiting the
/// endpoint's longest backoff between rounds.
fn feed(client: OutboundClient) -> Result<FeedRepository, OpenError> {
    let base = client.url().clone();
    let between_rounds = std::time::Duration::from(client.retry().max_backoff);
    let cleartext = base.scheme() == "http";
    let client: Arc<dyn FeedClient> = Arc::new(Feed(client));
    if cleartext {
        FeedRepository::cleartext(&base, client, between_rounds)
    } else {
        FeedRepository::new(&base, client, between_rounds)
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
