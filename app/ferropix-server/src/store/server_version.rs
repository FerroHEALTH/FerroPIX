// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The version gate: every job refuses a PostgreSQL older than the release
//! FerroPIX is built and tested on.
//!
//! The server reads `server_version_num` once when its pool starts
//! (PostgreSQL 19, "Preset Options",
//! <https://www.postgresql.org/docs/19/runtime-config-preset.html>). No
//! specification governs the minimum: our own design, the release every
//! FerroHEALTH product pins.

use crate::store::{Store, StoreError};

/// The oldest PostgreSQL the store runs on, as a `server_version_num`.
pub const MINIMUM: i32 = 190_000;

/// Refuses the store when it runs a PostgreSQL older than [`MINIMUM`].
///
/// # Errors
/// Returns [`StoreError::UnsupportedServer`] naming the version found and
/// the version required, and [`StoreError::Query`] when the setting cannot
/// be read.
pub async fn check(store: &Store) -> Result<(), StoreError> {
    let found: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::int")
        .fetch_one(store.pool())
        .await?;
    supported(found)
}

/// Refuses a `server_version_num` older than [`MINIMUM`].
fn supported(found: i32) -> Result<(), StoreError> {
    if found < MINIMUM {
        return Err(StoreError::UnsupportedServer {
            found,
            required: MINIMUM,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::supported;
    use crate::store::StoreError;

    #[test]
    fn a_server_older_than_postgresql_19_is_refused_with_both_versions() {
        let refused = supported(180_007).expect_err("PostgreSQL 18.7 is refused");
        assert!(matches!(
            refused,
            StoreError::UnsupportedServer {
                found: 180_007,
                required: 190_000
            }
        ));
        assert_eq!(
            "the store runs PostgreSQL server_version_num 180007, and FerroPIX needs 190000 or newer",
            refused.to_string()
        );
    }

    #[test]
    fn postgresql_19_and_newer_are_accepted() {
        for found in [190_000, 190_003, 200_000] {
            assert!(supported(found).is_ok(), "{found}");
        }
    }
}
