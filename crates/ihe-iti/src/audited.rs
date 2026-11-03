// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The audit record an audited FHIR client's answer waits on, under the
//! recording bounds of [`ihe_atna::balp::recording`].

use std::future::Future;

use ihe_atna::balp::AuditError;
use ihe_atna::balp::recording::{Late, Recorded, within};

/// Returns `result` once `record` is accepted, or the error `audit` makes of
/// the record's refusal, and of a record not accepted by `deadline` for an
/// answer that would be used.
pub(crate) async fn settled<T, E>(
    deadline: Option<tokio::time::Instant>,
    record: impl Future<Output = Result<(), AuditError>>,
    result: Result<T, E>,
    audit: impl FnOnce(AuditError) -> E,
) -> Result<T, E> {
    match within(deadline, record).await {
        Recorded::Refused(error) => Err(audit(error)),
        Recorded::Late if result.is_ok() => Err(audit(AuditError(Box::new(Late::new())))),
        Recorded::Accepted | Recorded::Late => result,
    }
}
