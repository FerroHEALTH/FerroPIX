// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! `db reseal`: every identifier column rewritten under a new key.
//!
//! The rotation reads the old key from the file `--old-key-file` names and
//! takes the new one from `[db] data_key_file`. It records the new key as
//! pending, which refuses every `serve` until it is done, then rewrites the
//! HMAC of every record, and of every identifier a version carries, and the
//! sealed value of every sealed one in batches, one transaction each,
//! walking each relation once by its key with a cursor: a run interrupted
//! keeps every batch it committed, and a resumed run walks again from the
//! first row, passing over each already under the new key.
//! The payloads waiting in the outbox, and the BALP records in the audit
//! outbox and its quarantine, are sealed the same way and are rewritten
//! after the identifiers.
//! When no row is left under the old key it records the new key as the
//! store's and drops the pending mark; the old key opens nothing afterwards.
//! No specification governs the rotation: our own design.

use secrecy::ExposeSecret as _;
use uuid::Uuid;

use crate::audit::outbox::AUDIT_RECORD;
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

/// One record as the rotation reads it: id, key id, domain, sealed value,
/// plain value.
type RecordRow = (Uuid, Vec<u8>, Uuid, Option<Vec<u8>>, Option<String>);

/// One carried identifier as the rotation reads it: its key (record,
/// version, domain, HMAC), its key id, then its sealed and plain value.
type CarriedRow = (
    Uuid,
    i32,
    Uuid,
    Vec<u8>,
    Vec<u8>,
    Option<Vec<u8>>,
    Option<String>,
);

/// The key of the last carried identifier a batch read, where the next
/// batch goes on.
type CarriedKey = (Uuid, i32, Uuid, Vec<u8>);

/// The statements that find a row left under another key than the store's.
const LEFT: [&str; 5] = [
    "SELECT key_id FROM pix.source_record WHERE key_id <> $1 LIMIT 1",
    "SELECT key_id FROM pix.carried_identifier WHERE key_id <> $1 LIMIT 1",
    "SELECT key_id FROM pix.outbox WHERE key_id <> $1 LIMIT 1",
    "SELECT key_id FROM pix.audit_outbox WHERE key_id <> $1 LIMIT 1",
    "SELECT key_id FROM pix.audit_quarantine WHERE key_id <> $1 LIMIT 1",
];

/// The tables of BALP records sealed under the store's key (#742).
const AUDIT_TABLES: [AuditTable; 2] = [
    AuditTable {
        select: "SELECT id, record_sealed FROM pix.audit_outbox WHERE key_id = $1 ORDER BY id LIMIT $2 FOR UPDATE",
        update: "UPDATE pix.audit_outbox SET record_sealed = $2, key_id = $3 WHERE id = $1",
    },
    AuditTable {
        select: "SELECT id, record_sealed FROM pix.audit_quarantine WHERE key_id = $1 ORDER BY id LIMIT $2 FOR UPDATE",
        update: "UPDATE pix.audit_quarantine SET record_sealed = $2, key_id = $3 WHERE id = $1",
    },
];

/// The statements that reseal one table of BALP records.
struct AuditTable {
    /// Reads a batch of records under the old key.
    select: &'static str,
    /// Writes one record under the new key.
    update: &'static str,
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
    let mut after = None;
    loop {
        let (rewritten, last) = reseal_records(store, old, new, batch, after).await?;
        let Some(last) = last else {
            break;
        };
        records = records.saturating_add(rewritten);
        after = Some(last);
        tracing::info!(rewritten, "records resealed in a batch");
    }
    let mut after = None;
    loop {
        let (rewritten, last) = reseal_carried(store, old, new, batch, after).await?;
        let Some(last) = last else {
            break;
        };
        after = Some(last);
        tracing::info!(rewritten, "carried identifiers resealed in a batch");
    }
    loop {
        let rewritten = reseal_outbox_batch(store, old, new, batch).await?;
        if rewritten == 0 {
            break;
        }
        tracing::info!(rewritten, "outbox payloads resealed in a batch");
    }
    for table in &AUDIT_TABLES {
        loop {
            let rewritten = reseal_audit_batch(store, old, new, batch, table).await?;
            if rewritten == 0 {
                break;
            }
            tracing::info!(rewritten, "audit records resealed in a batch");
        }
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

/// Opens the identifier of `domain` stored as `sealed` or `plain` under
/// `old`, and returns its HMAC and, for a sealed one, its seal under `new`.
fn resealed(
    old: &Sealer,
    new: &Sealer,
    domain: Uuid,
    sealed: Option<&[u8]>,
    plain: Option<String>,
) -> Result<([u8; 32], Option<Vec<u8>>), StoreError> {
    let value = match (sealed, plain) {
        (Some(sealed), _) => old.open(domain, sealed)?,
        (None, Some(plain)) => IdentifierValue::new(plain).ok_or(SealError::Malformed)?,
        (None, None) => return Err(StoreError::Seal(SealError::Malformed)),
    };
    let resealed = sealed
        .is_some()
        .then(|| new.seal(domain, &value))
        .transpose()?;
    Ok((new.hmac(domain, &value), resealed))
}

/// Rewrites the next batch of records after `after`, in id order, and
/// returns how many it rewrote and the id it read last; `None` once no
/// record follows.
///
/// The walk reads the primary key in order and passes over a record already
/// under the new key, so one pass covers the store, and a run resumed after
/// an interruption walks it again from the first.
async fn reseal_records(
    store: &Store,
    old: &Sealer,
    new: &Sealer,
    batch: i64,
    after: Option<Uuid>,
) -> Result<(u64, Option<Uuid>), StoreError> {
    let mut transaction = store.pool().begin().await?;
    let rows: Vec<RecordRow> = sqlx::query_as(
        "SELECT id, key_id, domain_id, identifier_sealed, identifier_value
         FROM pix.source_record WHERE $2::uuid IS NULL OR id > $2
         ORDER BY id LIMIT $1 FOR UPDATE",
    )
    .bind(batch)
    .bind(after)
    .fetch_all(&mut *transaction)
    .await?;
    let last = rows.last().map(|row| row.0);
    let mut rewritten: u64 = 0;
    for (id, key_id, domain, sealed, plain) in rows {
        if key_id.as_slice() != old.key_id().as_bytes().as_slice() {
            continue;
        }
        let (hmac, resealed) = resealed(old, new, domain, sealed.as_deref(), plain)?;
        sqlx::query(
            "UPDATE pix.source_record
             SET identifier_hmac = $2, identifier_sealed = COALESCE($3, identifier_sealed),
                 key_id = $4
             WHERE id = $1",
        )
        .bind(id)
        .bind(hmac.as_slice())
        .bind(resealed)
        .bind(new.key_id().as_bytes().as_slice())
        .execute(&mut *transaction)
        .await?;
        tracing::debug!(row = %id, "a record is resealed");
        rewritten = rewritten.saturating_add(1);
    }
    transaction.commit().await?;
    Ok((rewritten, last))
}

/// Rewrites the next batch of carried identifiers after `after`, in key
/// order, and returns how many it rewrote and the key it read last; `None`
/// once no identifier follows.
///
/// A rewritten identifier takes a new HMAC, which can sort after the cursor;
/// the walk passes over it as it is under the new key.
async fn reseal_carried(
    store: &Store,
    old: &Sealer,
    new: &Sealer,
    batch: i64,
    after: Option<CarriedKey>,
) -> Result<(u64, Option<CarriedKey>), StoreError> {
    let mut transaction = store.pool().begin().await?;
    let (record, version, domain, hmac) = match &after {
        Some((record, version, domain, hmac)) => (
            Some(*record),
            Some(*version),
            Some(*domain),
            Some(hmac.clone()),
        ),
        None => (None, None, None, None),
    };
    let rows: Vec<CarriedRow> = sqlx::query_as(
        "SELECT record_id, version, domain_id, identifier_hmac, key_id,
                    identifier_sealed, identifier_value
             FROM pix.carried_identifier
             WHERE $2::uuid IS NULL
                OR (record_id, version, domain_id, identifier_hmac) > ($2, $3, $4, $5)
             ORDER BY record_id, version, domain_id, identifier_hmac LIMIT $1 FOR UPDATE",
    )
    .bind(batch)
    .bind(record)
    .bind(version)
    .bind(domain)
    .bind(hmac)
    .fetch_all(&mut *transaction)
    .await?;
    let last = rows.last().map(|row| (row.0, row.1, row.2, row.3.clone()));
    let mut rewritten: u64 = 0;
    for (record, version, domain, hmac, key_id, sealed, plain) in rows {
        if key_id.as_slice() != old.key_id().as_bytes().as_slice() {
            continue;
        }
        let (new_hmac, resealed) = resealed(old, new, domain, sealed.as_deref(), plain)?;
        sqlx::query(
            "UPDATE pix.carried_identifier
             SET identifier_hmac = $5, identifier_sealed = COALESCE($6, identifier_sealed),
                 key_id = $7
             WHERE record_id = $1 AND version = $2 AND domain_id = $3 AND identifier_hmac = $4",
        )
        .bind(record)
        .bind(version)
        .bind(domain)
        .bind(hmac)
        .bind(new_hmac.as_slice())
        .bind(resealed)
        .bind(new.key_id().as_bytes().as_slice())
        .execute(&mut *transaction)
        .await?;
        rewritten = rewritten.saturating_add(1);
    }
    transaction.commit().await?;
    Ok((rewritten, last))
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

/// Reseals one batch of BALP records of `table` written under `old`, and
/// returns how many it rewrote.
async fn reseal_audit_batch(
    store: &Store,
    old: &Sealer,
    new: &Sealer,
    batch: i64,
    table: &AuditTable,
) -> Result<u64, StoreError> {
    let mut transaction = store.pool().begin().await?;
    let rows: Vec<(i64, Vec<u8>)> = sqlx::query_as(sqlx::AssertSqlSafe(table.select))
        .bind(old.key_id().as_bytes().as_slice())
        .bind(batch)
        .fetch_all(&mut *transaction)
        .await?;
    let mut rewritten: u64 = 0;
    for (id, sealed) in rows {
        let record = old.open_bytes(AUDIT_RECORD, &sealed)?;
        sqlx::query(sqlx::AssertSqlSafe(table.update))
            .bind(id)
            .bind(new.seal(AUDIT_RECORD, record.expose_secret())?)
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
    for statement in LEFT {
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
