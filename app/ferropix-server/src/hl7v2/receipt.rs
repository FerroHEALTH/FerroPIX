// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The receipt of each feed message: its sender, MSH-10 and acknowledgment.
//!
//! A message sent again is acknowledged again with the same code and not
//! applied twice (`docs/architecture.md` §7).
//!
//! The sender is named by MSH-3 and MSH-4 together, so two applications of
//! one facility never share a control id. A receipt is written once the
//! message settled, after its transaction committed; a message refused
//! because the store or the audit spool could not take it leaves none, so
//! the sender's retry is applied. No specification governs the receipts:
//! our own design.

use sqlx::PgPool;

use crate::store::StoreError;

/// Returns the code the message `control_id` of `sender` was acknowledged
/// with, `None` when it was not received before.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn answered(
    pool: &PgPool,
    sender: &str,
    control_id: &str,
) -> Result<Option<String>, StoreError> {
    Ok(sqlx::query_scalar(
        "SELECT ack_code FROM pix.v2_receipt WHERE sending_facility = $1 AND control_id = $2",
    )
    .bind(sender)
    .bind(control_id)
    .fetch_optional(pool)
    .await?)
}

/// Records that the message `control_id` of `sender` was acknowledged with
/// `code`; a receipt already written is kept.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn record(
    pool: &PgPool,
    sender: &str,
    control_id: &str,
    code: &str,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO pix.v2_receipt (control_id, sending_facility, ack_code)
         VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(control_id)
    .bind(sender)
    .bind(code)
    .execute(pool)
    .await?;
    Ok(())
}
