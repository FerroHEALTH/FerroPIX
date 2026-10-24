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

use sqlx::migrate::Migrator;

use crate::store::{Store, StoreError};

/// The schema every relation of the store lives in.
pub const SCHEMA: &str = "pix";

/// The bookkeeping table of the migrator, inside the schema it migrates.
const BOOKKEEPING: &str = "pix._sqlx_migrations";

/// Returns the migrator over the embedded set, which creates schema `pix`
/// and keeps its bookkeeping there.
#[must_use]
pub fn migrator() -> Migrator {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.dangerous_set_table_name(BOOKKEEPING);
    migrator.create_schema(SCHEMA);
    migrator
}

/// Applies the pending migrations, each in its own transaction, under the
/// migrator's advisory lock, so two servers starting together apply each
/// file once.
///
/// # Errors
/// Returns [`StoreError::Migrate`] when a migration fails, when an applied
/// one was changed since, or when the bookkeeping cannot be read.
pub async fn apply(store: &Store) -> Result<(), StoreError> {
    let migrator = migrator();
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
