// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The time an audited exchange's record is stored within.
//!
//! An audited client records each exchange before it returns, and the
//! `timeout` its caller gives the exchange bounds that record too: a recorder
//! that has not accepted the record when the time is up leaves the exchange
//! with no stored record. An exchange that succeeded then fails with [`Late`]
//! as its audit failure, because an answer is used only once its record is
//! stored. An exchange that failed reports its own failure, which the late
//! record would only hide; the recorder goes on storing it, so the record is
//! not lost. No specification governs this bound: our own design.

use std::time::Duration;

/// Why an audited exchange failed: its audit record was not stored before
/// the exchange's time was up, so its answer is not used.
#[derive(Debug, thiserror::Error)]
#[error("the audit record was not stored before the exchange's time was up")]
#[non_exhaustive]
pub struct Late;

/// What became of an exchange's audit record.
pub(crate) enum Recorded<E> {
    /// The recorder accepted it.
    Accepted,
    /// The recorder refused it.
    Refused(E),
    /// The recorder had not accepted it when the exchange's time was up.
    Late,
}

/// The end of `timeout` from now, or `None` past what the clock holds.
pub(crate) fn deadline(timeout: Duration) -> Option<tokio::time::Instant> {
    tokio::time::Instant::now().checked_add(timeout)
}

/// Awaits `record` until `deadline`.
pub(crate) async fn within<E>(
    deadline: Option<tokio::time::Instant>,
    record: impl Future<Output = Result<(), E>>,
) -> Recorded<E> {
    let outcome = match deadline {
        Some(deadline) => match tokio::time::timeout_at(deadline, record).await {
            Ok(outcome) => outcome,
            Err(_elapsed) => return Recorded::Late,
        },
        None => record.await,
    };
    match outcome {
        Ok(()) => Recorded::Accepted,
        Err(error) => Recorded::Refused(error),
    }
}
