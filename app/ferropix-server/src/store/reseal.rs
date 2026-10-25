// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! `db reseal`: every identifier column rewritten under a new key.
//!
//! The rotation reads the old key from the file `--old-key-file` names and
//! takes the new one from `[db] data_key_file`. It records the new key as
//! pending, which refuses every `serve` until it is done, then rewrites the
//! HMAC of every record, and of every identifier a version carries, and the
//! sealed value of every sealed one in batches,
//! one transaction each, so it resumes where an interrupted run stopped.
//! The payloads waiting in the outbox are sealed the same way and are
//! rewritten after the identifiers.
//! When no row is left under the old key it records the new key as the
//! store's and drops the pending mark; the old key opens nothing afterwards.
//! No specification governs the rotation: our own design.

use uuid::Uuid;

use crate::store::key::{self, StoredKey};
use crate::store::seal::{IdentifierValue, KeyId, SealError, Sealer};
use crate::store::{Store, StoreError};

/// The records one transaction of the rotation rewrites.
pub const BATCH: i64 = 500;

/// What a rotation did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResealReport {
    /// The key the rows were written under.
    pub from: KeyId,
    /// The key they are written under now.
    pub to: KeyId,
    /// The records rewritten by this run.
    pub records: u64,
}

/// One record as the rotation reads it.
type Row = (Uuid, Uuid, Option<Vec<u8>>, Option<String>);

/// The relations whose rows hold an identifier under the key: the records,
/// then the identifiers each version carries.
const TABLES: [Table; 2] = [Table::Record, Table::Carried];

/// A relation the rotation rewrites.
#[derive(Debug, Clone, Copy)]
enum Table {
    /// `pix.source_record`.
    Record,
    /// `pix.carried_identifier`.
    Carried,
}

impl Table {
    /// The statement that reads and locks one batch.
    const fn select(self) -> &'static str {
        match self {
            Self::Record => {
                "SELECT id, domain_id, identifier_sealed, identifier_value
                 FROM pix.source_record WHERE key_id = $1 ORDER BY id LIMIT $2 FOR UPDATE"
            }
            Self::Carried => {
                "SELECT id, domain_id, identifier_sealed, identifier_value
                 FROM pix.carried_identifier WHERE key_id = $1 ORDER BY id LIMIT $2 FOR UPDATE"
            }
        }
    }

    /// The statement that rewrites one row.
    const fn update(self) -> &'static str {
        match self {
            Self::Record => {
                "UPDATE pix.source_record
                 SET identifier_hmac = $2, identifier_sealed = COALESCE($3, identifier_sealed),
                     key_id = $4
                 WHERE id = $1"
            }
            Self::Carried => {
                "UPDATE pix.carried_identifier
                 SET identifier_hmac = $2, identifier_sealed = COALESCE($3, identifier_sealed),
                     key_id = $4
                 WHERE id = $1"
            }
        }
    }

    /// The statement that finds a row left under another key.
    const fn left(self) -> &'static str {
        match self {
            Self::Record => "SELECT key_id FROM pix.source_record WHERE key_id <> $1 LIMIT 1",
            Self::Carried => "SELECT key_id FROM pix.carried_identifier WHERE key_id <> $1 LIMIT 1",
        }
    }
}

/// Rewrites every record written under `old` under `new`, the key the store
/// holds.
///
/// # Errors
/// Returns [`StoreError::SameKey`] when both keys are one,
/// [`StoreError::OldKeyUnknown`] when the store is not written under the old
/// key (a finished reseal included),
/// [`StoreError::ResealPending`] when an unfinished reseal moves it to a
/// third key,
/// [`StoreError::Seal`] when a sealed value does not open under the old key,
/// and [`StoreError::Query`] when a statement fails, the batch in flight
/// rolled back.
pub async fn run(store: &Store, old: &Sealer, batch: i64) -> Result<ResealReport, StoreError> {
    let new = store.sealer();
    let (from, to) = (old.key_id(), new.key_id());
    if from == to {
        return Err(StoreError::SameKey);
    }
    begin(store, from, to).await?;
    let mut records: u64 = 0;
    for table in TABLES {
        loop {
            let rewritten = rewrite_batch(store, table, old, new, batch).await?;
            if rewritten == 0 {
                break;
            }
            if matches!(table, Table::Record) {
                records = records.saturating_add(rewritten);
            }
            tracing::info!(?table, rewritten, "identifiers resealed in a batch");
        }
    }
    loop {
        let rewritten = reseal_outbox_batch(store, old, new, batch).await?;
        if rewritten == 0 {
            break;
        }
        tracing::info!(rewritten, "outbox payloads resealed in a batch");
    }
    finish(store, to).await?;
    tracing::info!(from = %from, to = %to, records, "the store is resealed");
    Ok(ResealReport { from, to, records })
}

/// Marks `to` as the pending key, after checking the store names `from`.
async fn begin(store: &Store, from: KeyId, to: KeyId) -> Result<(), StoreError> {
    let mut transaction = store.pool().begin().await?;
    sqlx::query("SELECT 1 FROM pix.seal_key FOR UPDATE")
        .execute(&mut *transaction)
        .await?;
    match key::stored(&mut *transaction).await? {
        Some(StoredKey {
            key_id,
            pending: None,
        }) if key_id == from => {
            sqlx::query("UPDATE pix.seal_key SET pending_key_id = $1")
                .bind(to.as_bytes().as_slice())
                .execute(&mut *transaction)
                .await?;
        }
        Some(StoredKey {
            key_id,
            pending: Some(pending),
        }) if key_id == from && pending == to => {}
        Some(StoredKey {
            key_id,
            pending: Some(pending),
        }) => {
            return Err(StoreError::ResealPending {
                stored: key_id,
                pending,
            });
        }
        Some(StoredKey { key_id, .. }) => {
            return Err(StoreError::OldKeyUnknown {
                stored: key_id,
                old: from,
            });
        }
        None => {
            sqlx::query("INSERT INTO pix.seal_key (key_id, pending_key_id) VALUES ($1, $2)")
                .bind(from.as_bytes().as_slice())
                .bind(to.as_bytes().as_slice())
                .execute(&mut *transaction)
                .await?;
        }
    }
    transaction.commit().await?;
    Ok(())
}

/// Rewrites one batch of `table`'s rows still written under another key
/// than `new`'s, and returns how many it rewrote.
async fn rewrite_batch(
    store: &Store,
    table: Table,
    old: &Sealer,
    new: &Sealer,
    batch: i64,
) -> Result<u64, StoreError> {
    let mut transaction = store.pool().begin().await?;
    let rows: Vec<Row> = sqlx::query_as(table.select())
        .bind(old.key_id().as_bytes().as_slice())
        .bind(batch)
        .fetch_all(&mut *transaction)
        .await?;
    let mut rewritten: u64 = 0;
    for (id, domain, sealed, plain) in rows {
        let value = match (sealed.as_deref(), plain) {
            (Some(sealed), _) => old.open(domain, sealed)?,
            (None, Some(plain)) => IdentifierValue::new(plain).ok_or(SealError::Malformed)?,
            (None, None) => return Err(StoreError::Seal(SealError::Malformed)),
        };
        let resealed = sealed
            .is_some()
            .then(|| new.seal(domain, &value))
            .transpose()?;
        sqlx::query(table.update())
            .bind(id)
            .bind(new.hmac(domain, &value).as_slice())
            .bind(resealed)
            .bind(new.key_id().as_bytes().as_slice())
            .execute(&mut *transaction)
            .await?;
        tracing::debug!(row = %id, ?table, "an identifier is resealed");
        rewritten = rewritten.saturating_add(1);
    }
    transaction.commit().await?;
    Ok(rewritten)
}

/// Reseals one batch of outbox payloads written under `old`, each sealed for
/// its own row id, and returns how many it rewrote.
async fn reseal_outbox_batch(
    store: &Store,
    old: &Sealer,
    new: &Sealer,
    batch: i64,
) -> Result<u64, StoreError> {
    let mut transaction = store.pool().begin().await?;
    let rows: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(
        "SELECT id, payload_sealed FROM pix.outbox WHERE key_id = $1 ORDER BY id LIMIT $2 FOR UPDATE",
    )
    .bind(old.key_id().as_bytes().as_slice())
    .bind(batch)
    .fetch_all(&mut *transaction)
    .await?;
    let mut rewritten: u64 = 0;
    for (id, sealed) in rows {
        let payload = old.open(id, &sealed)?;
        sqlx::query("UPDATE pix.outbox SET payload_sealed = $2, key_id = $3 WHERE id = $1")
            .bind(id)
            .bind(new.seal(id, &payload)?)
            .bind(new.key_id().as_bytes().as_slice())
            .execute(&mut *transaction)
            .await?;
        rewritten = rewritten.saturating_add(1);
    }
    transaction.commit().await?;
    Ok(rewritten)
}

/// Records `to` as the store's key once no row is written under another.
async fn finish(store: &Store, to: KeyId) -> Result<(), StoreError> {
    let mut transaction = store.pool().begin().await?;
    for statement in TABLES
        .map(Table::left)
        .into_iter()
        .chain(["SELECT key_id FROM pix.outbox WHERE key_id <> $1 LIMIT 1"])
    {
        let left: Option<Vec<u8>> = sqlx::query_scalar(statement)
            .bind(to.as_bytes().as_slice())
            .fetch_optional(&mut *transaction)
            .await?;
        if let Some(stored) = left {
            return Err(StoreError::KeyMismatch {
                stored: KeyId::from_slice(&stored).ok_or(SealError::Malformed)?,
                configured: to,
            });
        }
    }
    sqlx::query("UPDATE pix.seal_key SET key_id = $1, pending_key_id = NULL, set_at = now()")
        .bind(to.as_bytes().as_slice())
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}
