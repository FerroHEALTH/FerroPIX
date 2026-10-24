// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The boot gate: `serve` refuses a role that can read any schema beside
//! `pix`.
//!
//! The server holds the identity of every patient it cross-references, so
//! its role must reach nothing else, and a role that reaches more is a
//! deployment fault to fix before the first request. The gate refuses a
//! superuser and a member of `pg_read_all_data`, which read every schema
//! present and future, and a role with `SELECT` on any table, view,
//! materialized view, foreign table or sequence outside `pix`, the system
//! catalogs aside (PostgreSQL 18, "Privileges",
//! <https://www.postgresql.org/docs/18/ddl-priv.html>). No specification
//! governs the gate: our own design, after FerroEHR's
//! `verify_domain_isolation`.

use crate::store::{Store, StoreError};

/// The first relation outside `pix` the connected role can read, as its
/// kind and its schema-qualified name.
const OTHER_SCHEMA_READ: &str = "
SELECT CASE c.relkind
           WHEN 'S' THEN 'sequence'
           WHEN 'v' THEN 'view'
           WHEN 'm' THEN 'materialized view'
           WHEN 'f' THEN 'foreign table'
           ELSE 'table'
       END,
       quote_ident(n.nspname) || '.' || quote_ident(c.relname)
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')
  AND n.nspname NOT IN ('pix', 'pg_catalog', 'information_schema')
  AND n.nspname NOT LIKE 'pg\\_toast%'
  AND n.nspname NOT LIKE 'pg\\_temp\\_%'
  AND CASE WHEN c.relkind = 'S'
           THEN pg_catalog.has_sequence_privilege(c.oid, 'SELECT, USAGE')
           ELSE pg_catalog.has_any_column_privilege(c.oid, 'SELECT')
      END
ORDER BY 2
LIMIT 1";

/// Refuses the connected role when it can read a schema beside `pix`.
///
/// # Errors
/// Returns [`StoreError::Superuser`], [`StoreError::ReadsAllData`] or
/// [`StoreError::ReadsOtherSchema`] naming the role and what it reaches, and
/// [`StoreError::Query`] when the catalog cannot be read.
pub async fn check(store: &Store) -> Result<(), StoreError> {
    let (role, superuser, reads_all): (String, bool, bool) = sqlx::query_as(
        "SELECT current_user::text, r.rolsuper,
                pg_catalog.pg_has_role(current_user, 'pg_read_all_data', 'MEMBER')
         FROM pg_catalog.pg_roles r
         WHERE r.rolname = current_user",
    )
    .fetch_one(store.pool())
    .await?;
    if superuser {
        return Err(StoreError::Superuser { role });
    }
    if reads_all {
        return Err(StoreError::ReadsAllData { role });
    }
    let breach: Option<(String, String)> = sqlx::query_as(OTHER_SCHEMA_READ)
        .fetch_optional(store.pool())
        .await?;
    if let Some((kind, relation)) = breach {
        return Err(StoreError::ReadsOtherSchema {
            role,
            kind,
            relation,
        });
    }
    tracing::info!(role = role.as_str(), "the role reads schema pix alone");
    Ok(())
}
