// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The outbox's rows: a message written before its request is acknowledged,
//! claimed by the sender when it falls due, and settled as delivered,
//! retried later, or abandoned once its attempts are spent.
//!
//! The payload names a patient, so it is sealed under the store's key with
//! the row's id as the associated data. A claim moves the row's `next_at`
//! ahead by a lease, so two senders on one store never deliver one row at
//! once, and a sender that stops mid-delivery leaves the row due again when
//! the lease runs out. No specification governs the outbox: our own design.

use std::fmt;
use std::time::Duration;

use secrecy::{ExposeSecret as _, SecretString};
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::store::seal::{IdentifierValue, SealError, Sealer};
use crate::store::{Store, StoreError};

/// How long a claimed row stays out of reach of another claim.
pub const LEASE: Duration = Duration::from_secs(300);

/// Who delivers a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// An XCPD asynchronous answer, posted to its reply endpoint.
    Xcpd,
}

impl Channel {
    /// The value the `channel` column holds.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Xcpd => "xcpd",
        }
    }

    /// The channel the `channel` column names.
    fn of(text: &str) -> Option<Self> {
        match text {
            "xcpd" => Some(Self::Xcpd),
            _ => None,
        }
    }
}

/// One message to write.
pub struct Message {
    /// Who delivers it.
    pub channel: Channel,
    /// Where it goes: a URL for an HTTP channel.
    pub destination: String,
    /// The media type of the payload.
    pub media_type: &'static str,
    /// The payload, which names a patient.
    pub payload: SecretString,
    /// The most attempts to deliver it.
    pub max_attempts: u32,
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Message")
            .field("channel", &self.channel)
            .field("media_type", &self.media_type)
            .field("max_attempts", &self.max_attempts)
            .finish_non_exhaustive()
    }
}

/// One claimed row, its payload opened.
pub struct Claimed {
    /// The row.
    pub id: Uuid,
    /// Who delivers it.
    pub channel: Channel,
    /// Where it goes.
    pub destination: String,
    /// The media type of the payload.
    pub media_type: String,
    /// The payload.
    pub payload: SecretString,
    /// The attempts made before this one.
    pub attempts: u32,
    /// The most attempts.
    pub max_attempts: u32,
}

impl fmt::Debug for Claimed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Claimed")
            .field("id", &self.id)
            .field("channel", &self.channel)
            .field("attempts", &self.attempts)
            .field("max_attempts", &self.max_attempts)
            .finish_non_exhaustive()
    }
}

/// A claimed row as it is read.
type Row = (Uuid, String, String, String, Vec<u8>, i32, i32);

/// Writes `message` to the outbox through `executor`, sealed under `sealer`,
/// and returns its id.
///
/// # Errors
/// Returns [`StoreError::Seal`] when the payload is empty or cannot be
/// sealed, and [`StoreError::Query`] when the statement fails.
pub async fn enqueue<'c, E>(
    executor: E,
    sealer: &Sealer,
    message: &Message,
) -> Result<Uuid, StoreError>
where
    E: PgExecutor<'c>,
{
    let id = Uuid::new_v4();
    let payload = IdentifierValue::new(message.payload.expose_secret().to_owned())
        .ok_or(SealError::Malformed)?;
    let ciphertext = sealer.seal(id, &payload)?;
    let attempts = i32::try_from(message.max_attempts).unwrap_or(i32::MAX);
    sqlx::query(
        "INSERT INTO pix.outbox
             (id, channel, destination, media_type, payload_sealed, key_id, max_attempts)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(message.channel.as_str())
    .bind(&message.destination)
    .bind(message.media_type)
    .bind(ciphertext)
    .bind(sealer.key_id().as_bytes().as_slice())
    .bind(attempts)
    .execute(executor)
    .await?;
    Ok(id)
}

/// Claims up to `limit` due rows for `lease`, oldest first, and opens their
/// payloads.
///
/// A row whose payload does not open under the store's key, or whose channel
/// no sender delivers, is abandoned with that reason and left out, so it
/// cannot hold back the rows behind it.
///
/// # Errors
/// Returns [`StoreError::Query`] when a statement fails.
pub async fn claim(store: &Store, limit: i64, lease: Duration) -> Result<Vec<Claimed>, StoreError> {
    let lease_ms = i64::try_from(lease.as_millis()).unwrap_or(i64::MAX);
    let rows: Vec<Row> = sqlx::query_as(
        "UPDATE pix.outbox SET next_at = now() + make_interval(secs => $2::double precision / 1000)
         WHERE id IN (
             SELECT id FROM pix.outbox
             WHERE delivered_at IS NULL AND abandoned_at IS NULL AND next_at <= now()
             ORDER BY next_at, id LIMIT $1 FOR UPDATE SKIP LOCKED)
         RETURNING id, channel, destination, media_type, payload_sealed, attempts, max_attempts",
    )
    .bind(limit)
    .bind(lease_ms)
    .fetch_all(store.pool())
    .await?;
    let mut claimed = Vec::with_capacity(rows.len());
    for (id, channel, destination, media_type, sealed, attempts, max_attempts) in rows {
        let opened =
            Channel::of(&channel).map(|channel| (channel, store.sealer().open(id, &sealed)));
        let (channel, payload) = match opened {
            Some((channel, Ok(payload))) => (channel, payload),
            Some((_, Err(_))) => {
                tracing::error!(row = %id, "an outbox message is abandoned: its payload does not open under the store's key");
                failed(
                    store,
                    id,
                    "the payload does not open under the store's key",
                    None,
                )
                .await?;
                continue;
            }
            None => {
                tracing::error!(row = %id, "an outbox message is abandoned: no sender delivers its channel");
                failed(store, id, "no sender delivers its channel", None).await?;
                continue;
            }
        };
        claimed.push(Claimed {
            id,
            channel,
            destination,
            media_type,
            payload: SecretString::from(payload.expose().to_owned()),
            attempts: u32::try_from(attempts).unwrap_or(0),
            max_attempts: u32::try_from(max_attempts).unwrap_or(1),
        });
    }
    claimed.sort_by_key(|row| row.id);
    Ok(claimed)
}

/// Returns how long until the next row falls due, `None` when none waits.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn next_due(store: &Store) -> Result<Option<Duration>, StoreError> {
    let wait: Option<f64> = sqlx::query_scalar(
        "SELECT greatest(extract(epoch FROM min(next_at) - now()), 0)::double precision
         FROM pix.outbox WHERE delivered_at IS NULL AND abandoned_at IS NULL",
    )
    .fetch_one(store.pool())
    .await?;
    Ok(wait.map(|seconds| Duration::from_secs_f64(seconds.clamp(0.0, 86_400.0))))
}

/// Records the row `id` as delivered.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn delivered(store: &Store, id: Uuid) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE pix.outbox SET delivered_at = now(), attempts = attempts + 1 WHERE id = $1",
    )
    .bind(id)
    .execute(store.pool())
    .await?;
    Ok(())
}

/// Records a failed attempt of the row `id` with `failure`, a text that names
/// no patient: the row falls due again after `retry`, or is abandoned when
/// `retry` is `None`.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn failed(
    store: &Store,
    id: Uuid,
    failure: &str,
    retry: Option<Duration>,
) -> Result<(), StoreError> {
    match retry {
        Some(wait) => {
            let wait_ms = i64::try_from(wait.as_millis()).unwrap_or(i64::MAX);
            sqlx::query(
                "UPDATE pix.outbox
                 SET attempts = attempts + 1, last_failure = $2,
                     next_at = now() + make_interval(secs => $3::double precision / 1000)
                 WHERE id = $1",
            )
            .bind(id)
            .bind(failure)
            .bind(wait_ms)
            .execute(store.pool())
            .await?;
        }
        None => {
            sqlx::query(
                "UPDATE pix.outbox
                 SET attempts = attempts + 1, last_failure = $2, abandoned_at = now()
                 WHERE id = $1",
            )
            .bind(id)
            .bind(failure)
            .execute(store.pool())
            .await?;
        }
    }
    Ok(())
}
