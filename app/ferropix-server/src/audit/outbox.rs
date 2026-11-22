// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The BALP records of the writes, kept in the store beside the change they
//! record (#742).
//!
//! A write inserts its records into `pix.audit_outbox` in its own
//! transaction, before COMMIT ([`insert`]), so the change and its records
//! commit together or not at all, and a crash between them leaves neither.
//! A refused write, which changed nothing, inserts its record in a statement
//! of its own. [`AuditOutbox`] is the `ihe-atna` forwarder's store over the
//! table: the oldest records go to the repository as one batch Bundle (ITI
//! TF-2 §3.20.4.4), each record the repository took is deleted, and one it
//! refused moves to `pix.audit_quarantine`, logged by its id and counted. A
//! crash after a send and before the delete sends that batch again, so each
//! record is delivered at least once, as the spool delivers. A record names a
//! patient, so it is compressed and sealed under the store's key (D7, D34).
//! No specification governs the storage: our own design.

use std::io::{Read as _, Write as _};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use ihe_atna::atna::forwarder::{Outbox, OutboxError};
use ihe_atna::atna::spool::{Depth, Stored};
use secrecy::{ExposeSecret as _, SecretSlice};
use sqlx::PgConnection;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::store::seal::{SealError, Sealer};
use crate::store::{Store, StoreError};

/// The associated data every record is sealed with: the table it belongs to.
///
/// Every row is a record like any other, so the associated data binds a
/// ciphertext to the table, and no sealed identifier of another table opens
/// as a record (no specification governs this: our own design).
pub const AUDIT_RECORD: Uuid = Uuid::from_u128(0x6175_6469_742d_7265_636f_7264_0000_0001);

/// How long the forwarder waits for a record before it looks again, when no
/// write in this process woke it (a write of another server on the store).
const LOOK_AGAIN: Duration = Duration::from_secs(1);

/// Why a record could not be kept in the outbox.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum KeepError {
    /// The audit trail was opened without the store its outbox is in.
    #[error("the audit trail has no store to keep a write's record in")]
    Unattached,
    /// The record could not be compressed.
    #[error("the audit record could not be compressed")]
    Compress(#[source] std::io::Error),
    /// The record could not be sealed.
    #[error("the audit record could not be sealed")]
    Seal(#[source] SealError),
    /// The statement failed.
    #[error("the audit record could not be inserted")]
    Store(#[source] StoreError),
}

/// Inserts `records` into the audit outbox through `connection`, in its
/// transaction when it holds one, sealed under `sealer`: one statement.
///
/// # Errors
/// Returns [`KeepError`] when a record cannot be compressed, sealed or
/// inserted.
pub async fn insert(
    connection: &mut PgConnection,
    sealer: &Sealer,
    records: &[SecretSlice<u8>],
) -> Result<(), KeepError> {
    if records.is_empty() {
        return Ok(());
    }
    let mut ciphertexts = Vec::with_capacity(records.len());
    for record in records {
        let compressed = compress(record.expose_secret()).map_err(KeepError::Compress)?;
        ciphertexts.push(
            sealer
                .seal(AUDIT_RECORD, compressed.as_slice())
                .map_err(KeepError::Seal)?,
        );
    }
    sqlx::query(
        "INSERT INTO pix.audit_outbox (record_sealed, key_id)
         SELECT record, $2 FROM unnest($1::bytea[]) WITH ORDINALITY AS r (record, n) ORDER BY n",
    )
    .bind(ciphertexts)
    .bind(sealer.key_id().as_bytes().as_slice())
    .execute(&mut *connection)
    .await
    .map_err(|error| KeepError::Store(error.into()))?;
    Ok(())
}

/// Returns `record` compressed with DEFLATE (RFC 1951).
fn compress(record: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::with_capacity(record.len()), Compression::default());
    encoder.write_all(record)?;
    encoder.finish()
}

/// Returns `compressed`, which [`compress`] wrote, as it was.
fn decompress(compressed: &[u8]) -> std::io::Result<SecretSlice<u8>> {
    let mut record = Vec::with_capacity(compressed.len().saturating_mul(3));
    DeflateDecoder::new(compressed).read_to_end(&mut record)?;
    Ok(SecretSlice::from(record))
}

/// The forwarder's store over the audit outbox of `store`.
#[derive(Debug, Clone)]
pub struct AuditOutbox {
    /// The store the table is in.
    store: Store,
    /// What wakes the forwarder once a write of this process committed.
    wake: Arc<Notify>,
    /// What the table held when the forwarder last read it.
    shown: Arc<Mutex<Depth>>,
}

impl AuditOutbox {
    /// Returns the forwarder's store over the audit outbox of `store`.
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self {
            store,
            wake: Arc::new(Notify::new()),
            shown: Arc::new(Mutex::new(Depth::default())),
        }
    }

    /// Returns the key the records are sealed under: the store's.
    #[must_use]
    pub fn sealer(&self) -> &Sealer {
        self.store.sealer()
    }

    /// Returns a connection of the store, for a record kept in a statement
    /// of its own.
    ///
    /// # Errors
    /// Returns [`StoreError::Query`] when the pool lends none.
    pub async fn connection(
        &self,
    ) -> Result<sqlx::pool::PoolConnection<sqlx::Postgres>, StoreError> {
        Ok(self.store.pool().acquire().await?)
    }

    /// Wakes the forwarder: a write committed records.
    pub fn committed(&self) {
        self.wake.notify_one();
    }

    /// Reads what the table holds and shows it to [`Outbox::depth`].
    ///
    /// # Errors
    /// Returns [`StoreError::Query`] when the statement fails.
    pub async fn refresh(&self) -> Result<Depth, StoreError> {
        let (waiting, waiting_bytes, quarantined, quarantined_bytes): (i64, i64, i64, i64) =
            sqlx::query_as(
                "SELECT (SELECT count(*) FROM pix.audit_outbox),
                        (SELECT coalesce(sum(octet_length(record_sealed)), 0)::bigint FROM pix.audit_outbox),
                        (SELECT count(*) FROM pix.audit_quarantine),
                        (SELECT coalesce(sum(octet_length(record_sealed)), 0)::bigint FROM pix.audit_quarantine)",
            )
            .fetch_one(self.store.pool())
            .await?;
        let count = |n: i64| usize::try_from(n).unwrap_or(usize::MAX);
        let bytes = |n: i64| u64::try_from(n).unwrap_or(u64::MAX);
        let quarantined = count(quarantined);
        let depth = Depth {
            messages: count(waiting).saturating_add(quarantined),
            bytes: bytes(waiting_bytes).saturating_add(bytes(quarantined_bytes)),
            quarantined,
        };
        *self.shown.lock().unwrap_or_else(PoisonError::into_inner) = depth;
        Ok(depth)
    }

    /// Moves the record `id` to the quarantine, logged by its id.
    async fn set_aside(&self, id: i64, why: &'static str) -> Result<(), StoreError> {
        sqlx::query(
            "WITH moved AS (DELETE FROM pix.audit_outbox WHERE id = $1 RETURNING id, record_sealed, key_id)
             INSERT INTO pix.audit_quarantine (id, record_sealed, key_id)
             SELECT id, record_sealed, key_id FROM moved",
        )
        .bind(id)
        .execute(self.store.pool())
        .await?;
        tracing::error!(
            id,
            reason = why,
            "an audit record was moved to the quarantine; the drain goes on"
        );
        Ok(())
    }

    /// Opens the sealed record `sealed`.
    fn open(&self, sealed: &[u8]) -> Result<SecretSlice<u8>, &'static str> {
        let compressed = self
            .store
            .sealer()
            .open_bytes(AUDIT_RECORD, sealed)
            .map_err(|_unopened| "it does not open under the store's key")?;
        decompress(compressed.expose_secret()).map_err(|_unread| "it does not decompress")
    }
}

/// Wraps a store failure for the forwarder.
fn failed(error: StoreError) -> OutboxError {
    OutboxError(Box::new(error))
}

#[async_trait::async_trait]
impl Outbox for AuditOutbox {
    async fn oldest(&self) -> Result<Option<Stored>, OutboxError> {
        Ok(self.oldest_batch(1).await?.into_iter().next())
    }

    async fn oldest_batch(&self, max: usize) -> Result<Vec<Stored>, OutboxError> {
        let limit = i64::try_from(max).unwrap_or(i64::MAX);
        let rows: Vec<(i64, Vec<u8>)> =
            sqlx::query_as("SELECT id, record_sealed FROM pix.audit_outbox ORDER BY id LIMIT $1")
                .bind(limit)
                .fetch_all(self.store.pool())
                .await
                .map_err(|error| failed(error.into()))?;
        let mut batch = Vec::with_capacity(rows.len());
        for (id, sealed) in rows {
            match (self.open(&sealed), u64::try_from(id)) {
                (Ok(message), Ok(sequence)) => batch.push(Stored { sequence, message }),
                (Err(why), _) => self.set_aside(id, why).await.map_err(failed)?,
                (_, Err(_negative)) => self
                    .set_aside(id, "its id is negative")
                    .await
                    .map_err(failed)?,
            }
        }
        self.refresh().await.map_err(failed)?;
        Ok(batch)
    }

    async fn stored(&self) {
        // NOTE: no specification governs this: our own design; a write of another
        // server on the store wakes no one here, so the forwarder looks again soon.
        let _woken_or_due = tokio::time::timeout(LOOK_AGAIN, self.wake.notified()).await;
    }

    async fn remove(&self, sequence: u64) -> Result<(), OutboxError> {
        let id = i64::try_from(sequence).map_err(|error| OutboxError(Box::new(error)))?;
        sqlx::query("DELETE FROM pix.audit_outbox WHERE id = $1")
            .bind(id)
            .execute(self.store.pool())
            .await
            .map_err(|error| failed(error.into()))?;
        Ok(())
    }

    async fn quarantine(&self, sequence: u64) -> Result<(), OutboxError> {
        let id = i64::try_from(sequence).map_err(|error| OutboxError(Box::new(error)))?;
        self.set_aside(id, "the audit repository refused it")
            .await
            .map_err(failed)?;
        self.refresh().await.map_err(failed)?;
        Ok(())
    }

    fn depth(&self) -> Depth {
        *self.shown.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_durable(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret as _;

    use super::{compress, decompress};

    #[test]
    fn a_record_decompresses_to_its_bytes() {
        let record = br#"{"resourceType":"AuditEvent","outcome":"0"}"#.repeat(20);
        let compressed = compress(&record).expect("it compresses");
        assert!(compressed.len() < record.len());
        assert_eq!(
            record.as_slice(),
            decompress(&compressed).expect("it reads").expose_secret()
        );
    }
}
