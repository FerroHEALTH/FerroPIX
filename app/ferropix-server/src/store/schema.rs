// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The migration set of schema `pix`, embedded at build time from
//! `app/ferropix-server/migrations/`.
//!
//! One concern per file, the grants in the last file of the set, applied in
//! order and recorded in `pix._sqlx_migrations`, so a second run applies
//! nothing and a file changed after it was applied is refused. No
//! specification governs the migrations: our own design
//! (`docs/architecture.md` §4.2).

use std::collections::BTreeSet;

use sqlx::migrate::Migrator;

use crate::store::{Store, StoreError, evidence};

/// The schema every relation of the store lives in.
pub const SCHEMA: &str = "pix";

/// The bookkeeping table of the migrator, inside the schema it migrates.
const BOOKKEEPING: &str = "pix._sqlx_migrations";

/// The migration that makes room for the packed evidence, after which the
/// server packs every row ([`evidence::refill`]).
const EVIDENCE_ROOM: i64 = 44;

/// The migration that drops the evidence stored as JSON.
const EVIDENCE_DONE: i64 = 45;

/// Returns the migrator over the embedded set, which creates schema `pix`
/// and keeps its bookkeeping there.
#[must_use]
pub fn migrator() -> Migrator {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.dangerous_set_table_name(BOOKKEEPING);
    migrator.create_schema(SCHEMA);
    migrator
}

/// Applies the pending migrations, packing the evidence between two of them.
///
/// Each migration runs in its own transaction under the migrator's
/// advisory lock, so two servers starting together apply each file once.
/// Between [`EVIDENCE_ROOM`] and [`EVIDENCE_DONE`] it packs the evidence
/// stored as JSON, which SQL cannot, so the set ends with every row in the
/// layout.
///
/// # Errors
/// Returns [`StoreError::Migrate`] when a migration fails, when an applied
/// one was changed since, or when the bookkeeping cannot be read, and every
/// refusal of [`evidence::refill`].
pub async fn apply(store: &Store) -> Result<(), StoreError> {
    let migrator = migrator();
    migrator
        .run_to(EVIDENCE_ROOM, store.pool())
        .await
        .map_err(StoreError::Migrate)?;
    if !applied(store).await?.contains(&EVIDENCE_DONE) {
        evidence::refill(store.pool(), evidence::BATCH).await?;
    }
    migrator
        .run(store.pool())
        .await
        .map_err(StoreError::Migrate)?;
    tracing::info!(
        migrations = migrator.iter().count(),
        schema = SCHEMA,
        "the store's migrations are applied"
    );
    Ok(())
}

/// The versions of the migrations applied to `store`.
async fn applied(store: &Store) -> Result<BTreeSet<i64>, StoreError> {
    let versions: Vec<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT version FROM {BOOKKEEPING} WHERE success"
    )))
    .fetch_all(store.pool())
    .await?;
    Ok(versions.into_iter().collect())
}

/// Refuses the start of a server that does not migrate while a migration
/// of its set is not applied to `store`.
///
/// # Errors
/// Returns [`StoreError::MigrationsPending`] naming how many are missing,
/// and [`StoreError::Query`] when the bookkeeping cannot be read.
pub async fn check(store: &Store) -> Result<(), StoreError> {
    let applied = applied(store).await?;
    let missing = migrator()
        .iter()
        .filter(|migration| !applied.contains(&migration.version))
        .count();
    if missing > 0 {
        return Err(StoreError::MigrationsPending { missing });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::migrator;

    #[test]
    fn the_grants_are_the_last_file_of_the_set() {
        let last = migrator().iter().last().map(|m| m.description.to_string());
        assert_eq!(
            Some(String::from("grants")),
            last,
            "a file after the grants escapes them"
        );
    }
}
