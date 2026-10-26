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
//! start. [`Backlog`] is its readiness indicator. An ITI-10 notification
//! goes over MLLP through `ferrobridge-hl7v2`'s sender ([`MllpChannel`]),
//! its acknowledgment read and checked (ITI TF-2 §3.10.4.1.2). No
//! specification governs the sender: our own design.

use std::sync::Arc;
use std::time::Duration;

use ferropix_operator_api::health::IndicatorState;
use secrecy::ExposeSecret as _;
use tokio::sync::{Notify, watch};

use crate::audit::Audit;
use crate::audit::records::{self, Identifier};
use crate::config::section::hl7v2::Consumer;
use crate::config::section::outbox::Retention;
use crate::hl7v2::authority::Authorities;
use crate::hl7v2::notification::{self, Acknowledged, Party};
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

/// The MLLP delivery of the ITI-10 notifications.
#[derive(Debug, Clone)]
pub struct MllpChannel {
    /// The consumers, each found by the name its notifications carry.
    pub consumers: Arc<[Consumer]>,
    /// The assigning authority of each domain, to read a notification back.
    pub authorities: Authorities,
    /// The largest acknowledgment one frame may carry.
    pub frame_limit: usize,
    /// The retry waits.
    pub backoff: Backoff,
    /// The audit trail each delivered notification is recorded on.
    pub audit: Audit,
}

/// The channels the sender delivers.
#[derive(Debug, Clone, Default)]
pub struct Channels {
    /// The XCPD asynchronous answers, when `[xcpd]` is on.
    pub xcpd: Option<HttpChannel>,
    /// The ITI-93 messages to PMIR subscribers, when `[pmir]` is on.
    pub pmir: Option<HttpChannel>,
    /// The ITI-10 notifications, when `[[hl7v2.consumers]]` names one.
    pub hl7v2: Option<MllpChannel>,
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

/// How long a due message may wait before the sender counts as stuck: a
/// lease and two idle wakes, past which a running sender has claimed it.
pub const STUCK: Duration = Duration::from_secs(LEASE.as_secs() + 2 * IDLE.as_secs());

/// The readiness indicator of the outbox sender's backlog.
///
/// It is down while a due message has waited past [`STUCK`], which a running
/// sender never lets happen. A subscriber that refuses its messages leaves them retried or
/// abandoned, never overdue, so it keeps the server ready.
#[derive(Debug, Clone)]
pub struct Backlog {
    /// The store whose outbox is read.
    store: Store,
}

impl Backlog {
    /// The name the indicator answers under in the readiness document.
    pub const NAME: &'static str = "outbox";

    /// Returns the indicator of `store`'s outbox.
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

impl crate::health::HealthIndicator for Backlog {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn check(&self) -> crate::health::Check<'_> {
        Box::pin(async move {
            match outbox::overdue(&self.store).await {
                Ok(Some(late)) if late > STUCK => IndicatorState::down(format!(
                    "a due outbox message has waited {} s; the sender claims none",
                    late.as_secs()
                )),
                Ok(_) => IndicatorState::up(),
                Err(error) => IndicatorState::down(format!(
                    "the outbox could not be read: {}",
                    crate::chain(&error)
                )),
            }
        })
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
    let (outcome, backoff) = match row.channel {
        Channel::Xcpd | Channel::Pmir => {
            let channel = match row.channel {
                Channel::Xcpd => channels.xcpd.as_ref(),
                _ => channels.pmir.as_ref(),
            };
            let outcome = match channel {
                Some(channel) => match headers(store, &row).await {
                    Ok(headers) => post(channel, &row, headers).await,
                    Err(failure) => Err(failure),
                },
                None => Err(String::from("the channel is switched off")),
            };
            (outcome, channel.map(|channel| channel.backoff))
        }
        Channel::Hl7v2 => match channels.hl7v2.as_ref() {
            Some(channel) => (notify(channel, &row).await, Some(channel.backoff)),
            None => (Err(String::from("the channel is switched off")), None),
        },
    };
    let attempt = row.attempts.saturating_add(1);
    let recorded = match outcome {
        Ok(()) => {
            tracing::info!(row = %row.id, channel = row.channel.as_str(), attempt, "an outbox message is delivered");
            outbox::delivered(store, row.id).await
        }
        Err(failure) => {
            let retry = (attempt < row.max_attempts)
                .then(|| backoff.map(|backoff| backoff.after(attempt)))
                .flatten();
            if retry.is_some() {
                tracing::warn!(row = %row.id, channel = row.channel.as_str(), attempt, failure = %failure, "an outbox message is not delivered; it is tried again");
            } else {
                tracing::error!(row = %row.id, channel = row.channel.as_str(), attempt, failure = %failure, "an outbox message is abandoned after its last attempt");
                subscription_failed(store, &row, &failure).await;
            }
            outbox::failed(store, row.id, &failure, retry).await
        }
    };
    if let Err(error) = recorded {
        tracing::error!(row = %row.id, error = %crate::chain(&error), "the outcome of an outbox delivery could not be recorded; the row falls due again after its lease");
    }
}

/// Sets the subscription of an abandoned ITI-93 row to `error` with the
/// failure, so the subscriber reads why it is sent nothing more (PMIR 1.6.0
/// §2:3.94.4.1.3); its later messages wait until it is enabled again.
async fn subscription_failed(store: &Store, row: &Claimed, failure: &str) {
    let Some(subscription) = row.subscription else {
        return;
    };
    let text = format!("an ITI-93 message could not be delivered: {failure}");
    match crate::store::subscription::failed(store, subscription, &text).await {
        Ok(true) => {
            tracing::warn!(subscription = %subscription, "a PMIR subscription is set to error");
        }
        Ok(false) => {}
        Err(error) => {
            tracing::error!(subscription = %subscription, error = %crate::chain(&error), "a PMIR subscription could not be set to error");
        }
    }
}

/// Sends the row's ITI-10 notification to its consumer over MLLP and reads
/// the acknowledgment: `AA` for this notification is delivered, anything
/// else a failure named by its kind, never the message's content.
///
/// The audit place is taken before the send, so a full spool keeps the
/// notification in the outbox, and the record is stored once the consumer
/// answered, `AA` a success and another code a minor failure (ITI TF-2
/// §3.10.5.1.1). A consumer that never answered received nothing to audit;
/// no specification governs that choice: our own design.
async fn notify(channel: &MllpChannel, row: &Claimed) -> Result<(), String> {
    let name = row.consumer.as_deref().unwrap_or(row.destination.as_str());
    let Some(consumer) = channel
        .consumers
        .iter()
        .find(|consumer| consumer.name == name)
    else {
        return Err(String::from(
            "no configured consumer has the notification's name",
        ));
    };
    let text = row.payload.expose_secret();
    let Some(written) = notification::read(text, &channel.authorities) else {
        return Err(String::from("the notification does not read back"));
    };
    let place = channel
        .audit
        .reserve()
        .map_err(|_full| String::from("the audit spool has no room for the record"))?;
    let timeouts = ferrobridge_hl7v2::mllp::SendTimeouts {
        connect: Duration::from_millis(consumer.connect_timeout_ms),
        reply: Duration::from_millis(consumer.reply_timeout_ms),
    };
    let reply = ferrobridge_hl7v2::mllp::send(
        consumer.address.as_str(),
        text.as_bytes(),
        ferrobridge_hl7v2::mllp::Codec::new(channel.frame_limit),
        timeouts,
    )
    .await
    .map_err(|error| sent_failure(&error))?;
    let answered = notification::acknowledged(&reply, &written.control_id);
    let outcome = if answered == Acknowledged::Accepted {
        ihe_atna::balp::Outcome::Success
    } else {
        ihe_atna::balp::Outcome::MinorFailure
    };
    let identifiers: Vec<Identifier> = written
        .identifiers
        .iter()
        .map(|(system, value)| Identifier {
            system: system.clone(),
            value: secrecy::SecretString::from(value.clone()),
        })
        .collect();
    let destination = Party {
        application: &consumer.application,
        facility: &consumer.facility,
    };
    let host = consumer
        .address
        .rsplit_once(':')
        .map_or(consumer.address.as_str(), |(host, _port)| host);
    let exchange = records::pix_update_notification(
        ihe_atna::balp::Peer {
            who: destination.audit_id(),
            network: ihe_atna::balp::NetworkAddress::host(host),
        },
        &written.control_id,
        &identifiers,
        outcome,
    );
    if let Err(error) = place.record(&exchange).await {
        tracing::error!(consumer = %consumer.name, error = %crate::chain(&error), "the audit record of an ITI-10 notification could not be stored");
    }
    match answered {
        Acknowledged::Accepted => Ok(()),
        Acknowledged::Refused(code) => Err(format!("the consumer acknowledged {code}")),
        Acknowledged::Unrelated(why) => Err(why.to_owned()),
    }
}

/// The failure text of an MLLP send that returned no reply, by its kind.
fn sent_failure(error: &ferrobridge_hl7v2::mllp::SendError) -> String {
    use ferrobridge_hl7v2::mllp::SendError;
    match error {
        SendError::Refused(_) => String::from("the consumer refused the connection"),
        SendError::ConnectTimedOut { .. } => String::from("the connection timed out"),
        SendError::ReplyTimedOut { .. } => String::from("the acknowledgment timed out"),
        SendError::Closed | SendError::Truncated => {
            String::from("the consumer closed the connection before its acknowledgment")
        }
        SendError::TooLarge { .. } => String::from("the acknowledgment runs past the frame limit"),
        SendError::Malformed { .. } => String::from("the acknowledgment is no MLLP frame"),
        SendError::Unframable => String::from("the notification cannot be framed"),
        SendError::Connect(_) | SendError::Io(_) => String::from("the connection failed"),
    }
}

/// The headers the row's subscription asks each message to carry (FHIR R4
/// `Subscription.channel.header`, each `name: value`), marked sensitive, as
/// one may carry the subscriber's credential; none for another channel.
async fn headers(store: &Store, row: &Claimed) -> Result<http::HeaderMap, String> {
    let mut map = http::HeaderMap::new();
    let Some(subscription) = row.subscription else {
        return Ok(map);
    };
    let written = crate::store::subscription::headers(store, subscription)
        .await
        .map_err(|_error| String::from("the subscription's headers could not be read"))?;
    for header in written {
        let parsed = header.split_once(':').and_then(|(name, value)| {
            let name = http::HeaderName::from_bytes(name.trim().as_bytes()).ok()?;
            let mut value = http::HeaderValue::from_str(value.trim()).ok()?;
            value.set_sensitive(true);
            Some((name, value))
        });
        let Some((name, value)) = parsed else {
            return Err(String::from("a channel header is no `name: value` field"));
        };
        map.append(name, value);
    }
    Ok(map)
}

/// Posts the row's payload to its destination with `headers`: a `2xx` is
/// delivered, and anything else a failure named by its status or its kind,
/// never its body.
async fn post(
    channel: &HttpChannel,
    row: &Claimed,
    headers: http::HeaderMap,
) -> Result<(), String> {
    let sent = channel
        .client
        .post(row.destination.as_str())
        .headers(headers)
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
