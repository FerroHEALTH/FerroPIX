// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The check that the configured sealing key is the key the store was
//! written under.
//!
//! A server started with another key would find no record by its HMAC and
//! answer every lookup as unknown, a silent wrong answer, so it refuses to
//! start instead. The first start records the key's id; a `db reseal` that
//! has not finished refuses every start until it does. No specification
//! governs this: our own design.

use crate::store::seal::KeyId;
use crate::store::seal::SealError;
use crate::store::{DbFailure, Store, StoreError};

/// The key the store names, and the key a running reseal moves it to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredKey {
    /// The key the identifier columns are written under.
    pub key_id: KeyId,
    /// The key an unfinished `db reseal` moves them to.
    pub pending: Option<KeyId>,
}

/// Reads the key the store names, or `None` before the first start.
///
/// # Errors
/// Returns [`StoreError::Query`] when the row cannot be read, and
/// [`StoreError::Seal`] when it holds a key id of the wrong length.
pub async fn stored<'c, E>(executor: E) -> Result<Option<StoredKey>, StoreError>
where
    E: sqlx::PgExecutor<'c>,
{
    let row: Option<(Vec<u8>, Option<Vec<u8>>)> =
        sqlx::query_as("SELECT key_id, pending_key_id FROM pix.seal_key")
            .fetch_optional(executor)
            .await?;
    row.map(|(key_id, pending)| {
        Ok(StoredKey {
            key_id: key_id_of(&key_id)?,
            pending: pending.as_deref().map(key_id_of).transpose()?,
        })
    })
    .transpose()
}

/// Returns the key id `bytes` hold.
fn key_id_of(bytes: &[u8]) -> Result<KeyId, StoreError> {
    KeyId::from_slice(bytes).ok_or(StoreError::Seal(SealError::Malformed))
}

/// Records the configured key on the first start, and refuses a start under
/// another key or during a reseal.
///
/// # Errors
/// Returns [`StoreError::ResealPending`] while a `db reseal` has not
/// finished, [`StoreError::KeyMismatch`] when the store names another key,
/// and [`StoreError::Query`] when the row cannot be read or written.
pub async fn verify(store: &Store) -> Result<(), StoreError> {
    let configured = store.sealer().key_id();
    sqlx::query("INSERT INTO pix.seal_key (key_id) VALUES ($1) ON CONFLICT (singleton) DO NOTHING")
        .bind(configured.as_bytes().as_slice())
        .execute(store.pool())
        .await?;
    let stored = stored(store.pool())
        .await?
        .ok_or(StoreError::Query(DbFailure(sqlx::Error::RowNotFound)))?;
    if let Some(pending) = stored.pending {
        return Err(StoreError::ResealPending {
            stored: stored.key_id,
            pending,
        });
    }
    if stored.key_id != configured {
        return Err(StoreError::KeyMismatch {
            stored: stored.key_id,
            configured,
        });
    }
    tracing::info!(key = %configured, "the store is written under the configured key");
    Ok(())
}
