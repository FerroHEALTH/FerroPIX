// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The store-and-forward sender of ITI-20 messages (ITI TF-2 §3.20.4.1.1).
//!
//! Every message is stored in the [`Spool`] first and delivered from it,
//! oldest first, so a message counts as recorded once it is stored, and a
//! repository that cannot be reached delays its delivery without losing it.
//! [`Forwarder::submit`] only ever writes to the spool: it never waits on
//! the network, so a slow repository never holds an exchange.
//!
//! [`Forwarder::run`] is the delivery loop: it keeps one connection open,
//! writes each stored message to it within the repository's timeouts,
//! removes the message once it is written, and after any transport failure
//! drops the connection and retries with an exponential backoff, with
//! jitter, capped at the configured longest wait.

use std::hash::BuildHasher as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use secrecy::SecretSlice;
use tokio::sync::Notify;

use super::repository::{Connection, Repository};
use super::spool::{Depth, Spool, SpoolError};

/// The first wait after a failed delivery.
const FIRST_RETRY: Duration = Duration::from_millis(250);

/// What the forwarder reports of itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// What the spool holds, awaiting delivery or in quarantine.
    pub depth: Depth,
    /// The messages delivered since the forwarder started.
    pub delivered: u64,
    /// The failed delivery attempts since the forwarder started, each
    /// followed by a backoff.
    pub retries: u64,
    /// Whether the last attempt to deliver succeeded; `false` while the
    /// forwarder retries.
    pub reachable: bool,
    /// Whether the spool is on disk.
    pub durable: bool,
}

/// The store-and-forward sender to one Audit Record Repository.
#[derive(Debug)]
pub struct Forwarder {
    spool: Spool,
    repository: Repository,
    retry_max: Duration,
    wake: Notify,
    delivered: AtomicU64,
    retries: AtomicU64,
    reachable: AtomicBool,
}

impl Forwarder {
    /// The sender of the messages in `spool` to `repository`, waiting at
    /// most `retry_max` between two attempts.
    #[must_use]
    pub fn new(spool: Spool, repository: Repository, retry_max: Duration) -> Arc<Self> {
        Arc::new(Self {
            spool,
            repository,
            retry_max: retry_max.max(FIRST_RETRY),
            wake: Notify::new(),
            delivered: AtomicU64::new(0),
            retries: AtomicU64::new(0),
            reachable: AtomicBool::new(true),
        })
    }

    /// Stores `frame` for delivery, returning once it is stored.
    ///
    /// # Errors
    ///
    /// The [`SpoolError`] of a spool that is full or cannot be written: the
    /// message is then neither stored nor delivered.
    pub async fn submit(&self, frame: SecretSlice<u8>) -> Result<(), SpoolError> {
        self.spool.push(frame).await?;
        self.wake.notify_one();
        Ok(())
    }

    /// What the forwarder holds and how its deliveries went.
    #[must_use]
    pub fn status(&self) -> Status {
        Status {
            depth: self.spool.depth(),
            delivered: self.delivered.load(Ordering::Relaxed),
            retries: self.retries.load(Ordering::Relaxed),
            reachable: self.reachable.load(Ordering::Relaxed),
            durable: self.spool.is_durable(),
        }
    }

    /// Delivers the stored messages, oldest first, for as long as it runs.
    ///
    /// The caller spawns it once and aborts the task to stop it; a message
    /// in flight then stays stored and is delivered again by the next run.
    pub async fn run(self: Arc<Self>) {
        let mut connection: Option<Connection> = None;
        let mut retry = FIRST_RETRY;
        loop {
            let stored = match self.spool.oldest().await {
                Ok(Some(stored)) => stored,
                Ok(None) => {
                    self.wake.notified().await;
                    continue;
                }
                Err(error) => {
                    tracing::warn!(error = %chain(&error), "the audit spool could not be read");
                    retry = self.back_off(retry).await;
                    continue;
                }
            };
            let reusable = match connection.take() {
                Some(mut open) => (!open.is_closed().await).then_some(open),
                None => None,
            };
            let mut open = match reusable {
                Some(open) => open,
                None => match self.repository.connect().await {
                    Ok(open) => open,
                    Err(error) => {
                        tracing::warn!(
                            error = %chain(&error),
                            "the audit repository could not be reached; its messages stay spooled"
                        );
                        retry = self.back_off(retry).await;
                        continue;
                    }
                },
            };
            if let Err(error) = open.send(&stored.message).await {
                tracing::warn!(
                    error = %chain(&error),
                    "an audit message could not be delivered; the connection is dropped and the message stays spooled"
                );
                retry = self.back_off(retry).await;
                continue;
            }
            connection = Some(open);
            self.reachable.store(true, Ordering::Relaxed);
            retry = FIRST_RETRY;
            match self.spool.remove(stored.sequence).await {
                Ok(()) => {
                    self.delivered.fetch_add(1, Ordering::Relaxed);
                }
                Err(error) => {
                    tracing::warn!(
                        error = %chain(&error),
                        "a delivered audit message could not be removed from the spool"
                    );
                    retry = self.back_off(retry).await;
                }
            }
        }
    }

    /// Records a failed attempt, waits about `retry`, and returns the wait
    /// after it: twice as long, at most the configured longest wait.
    async fn back_off(&self, retry: Duration) -> Duration {
        self.reachable.store(false, Ordering::Relaxed);
        self.retries.fetch_add(1, Ordering::Relaxed);
        tokio::time::sleep(jittered(retry)).await;
        retry.saturating_mul(2).min(self.retry_max)
    }
}

/// A wait between half of `retry` and `retry`, so senders that failed
/// together do not retry together (no specification governs this: our own
/// design).
fn jittered(retry: Duration) -> Duration {
    let half = retry / 2;
    let spread = u64::try_from(half.as_nanos()).unwrap_or(u64::MAX);
    let random = std::collections::hash_map::RandomState::new().hash_one(Instant::now());
    half.saturating_add(Duration::from_nanos(random % spread.saturating_add(1)))
}

/// `error` with its causes, joined.
fn chain(error: &dyn std::error::Error) -> String {
    let mut line = error.to_string();
    let mut cause = error.source();
    while let Some(source) = cause {
        line.push_str(": ");
        line.push_str(&source.to_string());
        cause = source.source();
    }
    line
}
