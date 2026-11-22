// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The refill of every head whose scoring features are not of this
//! server's [`FEATURES_VERSION`], and the check a server that does not
//! migrate runs instead.
//!
//! The features and the blocking keys come from the server's normalisation,
//! which SQL cannot run, so the server writes them after the migrations: a
//! thousand heads at a time, each transaction holding the advisory lock of
//! each record's identifier as a feed that moves the head does, so a head
//! is never refilled from a version a concurrent feed has replaced. A record
//! a feed holds is left to a later pass, and the refill ends when no stale
//! head is left. No specification governs the features: our own design
//! (`docs/architecture.md` §5.2).

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgPool;
use uuid::Uuid;

use crate::matching::features::{FEATURES_VERSION, FeaturesError};
use crate::matching::subject::Subject;
use crate::store::record::{self, Blocks};
use crate::store::{StoreError, demographics};

/// The heads one transaction of the refill rewrites.
pub const BATCH: i64 = 1000;

/// The pause before the refill passes again over heads a feed held.
const HELD_PAUSE: std::time::Duration = std::time::Duration::from_millis(50);

/// Rewrites the keys and the features of every head in `pool` whose
/// features are not of [`FEATURES_VERSION`], `batch` heads per transaction,
/// and returns how many it rewrote.
///
/// # Errors
/// Returns [`StoreError::Query`] when a statement fails,
/// [`StoreError::Demographics`] when a head's demographics are no
/// `Patient`, and [`StoreError::Features`] when its features cannot be
/// written.
pub async fn refill(pool: &PgPool, batch: i64) -> Result<u64, StoreError> {
    let mut rewritten: u64 = 0;
    loop {
        let mut after = Uuid::nil();
        let mut held = false;
        loop {
            let (last, done, skipped) = pass(pool, after, batch).await?;
            rewritten = rewritten.saturating_add(done);
            held |= skipped;
            match last {
                Some(last) => after = last,
                None => break,
            }
        }
        if !held {
            if rewritten > 0 {
                tracing::info!(
                    heads = rewritten,
                    "the record heads' scoring features were refilled"
                );
            }
            return Ok(rewritten);
        }
        tokio::time::sleep(HELD_PAUSE).await;
    }
}

/// Rewrites the stale heads after `after`, at most `batch`, in one
/// transaction; returns the last record it read, how many it rewrote and
/// whether a feed held one of them.
async fn pass(
    pool: &PgPool,
    after: Uuid,
    batch: i64,
) -> Result<(Option<Uuid>, u64, bool), StoreError> {
    let mut transaction = pool.begin().await?;
    let stale: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(
        "SELECT b.record_id, r.identifier_hmac
         FROM pix.record_block b
         JOIN pix.source_record r ON r.id = b.record_id
         WHERE b.record_id > $1 AND b.features_version IS DISTINCT FROM $2
         ORDER BY b.record_id
         LIMIT $3",
    )
    .bind(after)
    .bind(FEATURES_VERSION)
    .bind(batch)
    .fetch_all(&mut *transaction)
    .await?;
    let Some(last) = stale.last().map(|(id, _)| *id) else {
        return Ok((None, 0, false));
    };
    let mut by_key: BTreeMap<i64, Vec<Uuid>> = BTreeMap::new();
    for (id, hmac) in &stale {
        let hmac: &[u8; 32] = hmac
            .as_slice()
            .try_into()
            .map_err(|_short| StoreError::Seal(crate::store::seal::SealError::Malformed))?;
        by_key
            .entry(record::identity_key(hmac))
            .or_default()
            .push(*id);
    }
    let keys: Vec<i64> = by_key.keys().copied().collect();
    // NOTE: no specification governs this: our own design; a try-lock never waits, so the
    // refill cannot deadlock with a feed, and a record a feed holds waits for a later pass.
    let taken: Vec<(i64, bool)> = sqlx::query_as(
        "SELECT key, pg_try_advisory_xact_lock(key) FROM unnest($1::bigint[]) AS key ORDER BY key",
    )
    .bind(&keys)
    .fetch_all(&mut *transaction)
    .await?;
    let skipped = taken.iter().any(|(_, locked)| !locked);
    let ids: Vec<Uuid> = taken
        .iter()
        .filter(|(_, locked)| *locked)
        .filter_map(|(key, _)| by_key.get(key))
        .flatten()
        .copied()
        .collect();
    let heads: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT r.id, v.demographics::text
         FROM pix.source_record r
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
         WHERE r.id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&mut *transaction)
    .await?;
    let mut records = Vec::with_capacity(heads.len());
    let mut phonetic = Vec::with_capacity(heads.len());
    let mut postal = Vec::with_capacity(heads.len());
    let mut features = Vec::with_capacity(heads.len());
    for (id, text) in heads {
        let patient = demographics::patient(&text)?;
        let blocks = Blocks::of(&Subject::from_patient(&patient, BTreeSet::new()))?;
        records.push(id);
        phonetic.push(blocks.phonetic);
        postal.push(blocks.postal);
        features.push(blocks.features);
    }
    let done = sqlx::query(
        "UPDATE pix.record_block b
         SET block_phonetic = n.phonetic, block_postal = n.postal,
             features = n.features, features_version = $5
         FROM unnest($1::uuid[], $2::text[], $3::text[], $4::text[])
             AS n (record_id, phonetic, postal, features)
         WHERE b.record_id = n.record_id",
    )
    .bind(&records)
    .bind(&phonetic)
    .bind(&postal)
    .bind(&features)
    .bind(FEATURES_VERSION)
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    transaction.commit().await?;
    Ok((Some(last), done, skipped))
}

/// Refuses the start of a server that does not migrate while a head in
/// `pool` holds features of another version than [`FEATURES_VERSION`].
///
/// # Errors
/// Returns [`StoreError::Features`] naming the version a stale head holds,
/// and [`StoreError::Query`] when the statement fails.
pub async fn check(pool: &PgPool) -> Result<(), StoreError> {
    let stale: Option<Option<i16>> = sqlx::query_scalar(
        "SELECT features_version FROM pix.record_block
         WHERE features_version IS DISTINCT FROM $1
         LIMIT 1",
    )
    .bind(FEATURES_VERSION)
    .fetch_optional(pool)
    .await?;
    match stale {
        Some(found) => Err(StoreError::Features(FeaturesError::Stale { found })),
        None => Ok(()),
    }
}
