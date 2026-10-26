// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The outbox sender: one task that delivers every due message of the
//! outbox, retries a failed one with a doubling wait, and abandons it once
//! its attempts are spent.
//!
//! The sender wakes when a message is written, when the next one falls due,
//! and at least every [`IDLE`]. Every [`PRUNE_EVERY`] it deletes the
//! delivered and abandoned messages older than the `[outbox]` retention,
//! never a pending one. On `SIGTERM` it finishes the message in
//! flight and claims no other; the server waits for it within its drain
//! timeout, and a message it never reached stays in the outbox for the next
//! start. No specification governs the sender: our own design.

use std::sync::Arc;
use std::time::Duration;

use secrecy::ExposeSecret as _;
use tokio::sync::{Notify, watch};

use crate::config::section::outbox::Retention;
use crate::store::Store;
use crate::store::outbox::{self, Channel, Claimed, LEASE};

/// The longest the sender sleeps without looking at the outbox.
pub const IDLE: Duration = Duration::from_secs(30);

/// How often the sender prunes the settled messages past their retention.
pub const PRUNE_EVERY: Duration = Duration::from_secs(3600);

/// The rows one claim takes.
const BATCH: i64 = 16;

/// The wait before the first retry, and the longest between two, of a
/// channel's messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    /// The wait before the first retry.
    pub initial: Duration,
    /// The longest wait between two attempts.
    pub max: Duration,
}

impl Backoff {
    /// The wait after the `attempts`-th failed attempt: `initial` doubled per
    /// earlier failure, at most `max`.
    #[must_use]
    pub fn after(self, attempts: u32) -> Duration {
        let doublings = attempts.saturating_sub(1).min(32);
        self.initial
            .saturating_mul(2_u32.saturating_pow(doublings))
            .min(self.max)
    }
}

/// The HTTP delivery of one channel.
#[derive(Debug, Clone)]
pub struct HttpChannel {
    /// The client, with the channel's timeout and identity.
    pub client: reqwest::Client,
    /// The retry waits.
    pub backoff: Backoff,
}

/// The channels the sender delivers.
#[derive(Debug, Clone, Default)]
pub struct Channels {
    /// The XCPD asynchronous answers, when `[xcpd]` is on.
    pub xcpd: Option<HttpChannel>,
}

/// What wakes the sender: a message written.
#[derive(Debug, Clone, Default)]
pub struct Wake(Arc<Notify>);

impl Wake {
    /// Wakes the sender to look at the outbox now.
    pub fn now(&self) {
        self.0.notify_one();
    }
}

/// Starts the sender over `store`, delivering through `channels` and pruning
/// by `retention`, woken by `wake`, until `stopped` is raised.
#[must_use]
pub fn start(
    store: Store,
    channels: Channels,
    retention: Retention,
    wake: Wake,
    stopped: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(run(store, channels, retention, wake, stopped))
}

/// The sender's loop.
async fn run(
    store: Store,
    channels: Channels,
    retention: Retention,
    wake: Wake,
    mut stopped: watch::Receiver<bool>,
) {
    let mut pruned: Option<tokio::time::Instant> = None;
    loop {
        if *stopped.borrow() {
            return;
        }
        if pruned.is_none_or(|at| at.elapsed() >= PRUNE_EVERY) {
            prune(&store, retention).await;
            pruned = Some(tokio::time::Instant::now());
        }
        match outbox::claim(&store, BATCH, LEASE).await {
            Ok(claimed) => {
                for row in claimed {
                    deliver(&store, &channels, row).await;
                    if *stopped.borrow() {
                        tracing::info!("the outbox sender stops: the server is draining");
                        return;
                    }
                }
            }
            Err(error) => {
                tracing::error!(error = %crate::chain(&error), "the outbox could not be read");
            }
        }
        let wait = match outbox::next_due(&store).await {
            Ok(Some(due)) => due.min(IDLE),
            Ok(None) => IDLE,
            Err(error) => {
                tracing::error!(error = %crate::chain(&error), "the outbox could not be read");
                IDLE
            }
        };
        if wait.is_zero() {
            continue;
        }
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = wake.0.notified() => {}
            _ = stopped.wait_for(|stop| *stop) => return,
        }
    }
}

/// Deletes every settled message past `retention`, a batch at a time.
async fn prune(store: &Store, retention: Retention) {
    let mut total: u64 = 0;
    loop {
        match outbox::prune(store, retention.keep, retention.batch).await {
            Ok(deleted) => {
                total = total.saturating_add(deleted);
                if deleted < u64::from(retention.batch) {
                    break;
                }
            }
            Err(error) => {
                tracing::error!(error = %crate::chain(&error), "the outbox could not be pruned");
                break;
            }
        }
    }
    if total > 0 {
        tracing::info!(
            deleted = total,
            "settled outbox messages past their retention are pruned"
        );
    }
}

/// Delivers one claimed row and records how it went.
async fn deliver(store: &Store, channels: &Channels, row: Claimed) {
    let channel = match row.channel {
        Channel::Xcpd => channels.xcpd.as_ref(),
    };
    let attempt = row.attempts.saturating_add(1);
    let outcome = match channel {
        Some(channel) => post(channel, &row).await,
        None => Err(String::from("the channel is switched off")),
    };
    let recorded = match outcome {
        Ok(()) => {
            tracing::info!(row = %row.id, channel = row.channel.as_str(), attempt, "an outbox message is delivered");
            outbox::delivered(store, row.id).await
        }
        Err(failure) => {
            let retry = (attempt < row.max_attempts)
                .then(|| channel.map(|channel| channel.backoff.after(attempt)))
                .flatten();
            if retry.is_some() {
                tracing::warn!(row = %row.id, channel = row.channel.as_str(), attempt, failure = %failure, "an outbox message is not delivered; it is tried again");
            } else {
                tracing::error!(row = %row.id, channel = row.channel.as_str(), attempt, failure = %failure, "an outbox message is abandoned after its last attempt");
            }
            outbox::failed(store, row.id, &failure, retry).await
        }
    };
    if let Err(error) = recorded {
        tracing::error!(row = %row.id, error = %crate::chain(&error), "the outcome of an outbox delivery could not be recorded; the row falls due again after its lease");
    }
}

/// Posts the row's payload to its destination: a `2xx` is delivered, and
/// anything else a failure named by its status or its kind, never its body.
async fn post(channel: &HttpChannel, row: &Claimed) -> Result<(), String> {
    let sent = channel
        .client
        .post(row.destination.as_str())
        .header(http::header::CONTENT_TYPE, row.media_type.as_str())
        .body(row.payload.expose_secret().to_owned())
        .send()
        .await;
    match sent {
        Ok(response) if response.status().is_success() => Ok(()),
        Ok(response) => Err(format!("the endpoint answered {}", response.status())),
        Err(error) if error.is_timeout() => Err(String::from("the attempt timed out")),
        Err(error) if error.is_connect() => {
            Err(String::from("the endpoint refused the connection"))
        }
        Err(_) => Err(String::from("the request failed")),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Backoff;

    #[test]
    fn the_wait_doubles_from_the_first_retry_up_to_its_bound() {
        let backoff = Backoff {
            initial: Duration::from_millis(100),
            max: Duration::from_millis(500),
        };
        assert_eq!(
            vec![100, 200, 400, 500, 500],
            (1..=5)
                .map(|attempt| u64::try_from(backoff.after(attempt).as_millis()).expect("small"))
                .collect::<Vec<_>>()
        );
    }
}
