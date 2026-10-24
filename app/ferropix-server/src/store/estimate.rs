// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The field frequencies `ferropix matching estimate` reads.
//!
//! Over the current version of every active record, it counts how many
//! records hold each value of each field, and each identifier carried in a
//! `local` domain. Only the counts leave this module. No specification governs the
//! estimate: our own design (`docs/architecture.md` §5.2).

use std::collections::BTreeMap;

use crate::config::section::matching::Field;
use crate::matching::estimate;
use crate::matching::scorer::FIELDS;
use crate::matching::subject::Subject;
use crate::store::{Store, StoreError};

/// The records one page of the count reads.
const PAGE: i64 = 5_000;

/// The estimated `u` of each field that enough records state, with the
/// number of records that state it.
pub type Estimates = BTreeMap<Field, (f64, u64)>;

/// Counts the values of every field over the current versions and returns
/// the `u` of each field enough records state.
///
/// A version whose demographics do not read as a Patient is counted for no
/// field.
///
/// # Errors
/// Returns [`StoreError::Query`] when a statement fails.
pub async fn estimate(store: &Store) -> Result<Estimates, StoreError> {
    let mut counts: BTreeMap<Field, BTreeMap<String, u64>> = BTreeMap::new();
    let mut unreadable: u64 = 0;
    let mut after = uuid::Uuid::nil();
    loop {
        let page: Vec<(uuid::Uuid, String)> = sqlx::query_as(
            "SELECT r.id, v.demographics::text
             FROM pix.source_record r
             JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
             WHERE r.active AND r.removed_at IS NULL AND r.id > $1
             ORDER BY r.id LIMIT $2",
        )
        .bind(after)
        .bind(PAGE)
        .fetch_all(store.pool())
        .await?;
        let Some((last, _)) = page.last() else {
            break;
        };
        after = *last;
        for (_, text) in page {
            let Ok(patient) = crate::feed::demographics::patient(&text) else {
                unreadable = unreadable.saturating_add(1);
                continue;
            };
            let subject = Subject::from_patient(&patient, std::collections::BTreeSet::new());
            for field in FIELDS {
                if let Some(value) = estimate::value(field, &subject) {
                    let count = counts.entry(field).or_default().entry(value).or_insert(0);
                    *count = count.saturating_add(1);
                }
            }
        }
    }
    if unreadable > 0 {
        tracing::warn!(
            versions = unreadable,
            "versions whose demographics do not read are left out of the estimate"
        );
    }
    let local: Vec<i64> = sqlx::query_scalar(
        "SELECT count(*)
         FROM pix.carried_identifier i
         JOIN pix.source_record r ON r.id = i.record_id AND i.version = r.head_version
         JOIN pix.identifier_domain d ON d.id = i.domain_id
         WHERE d.kind = 'local' AND r.active AND r.removed_at IS NULL
         GROUP BY i.domain_id, i.identifier_hmac",
    )
    .fetch_all(store.pool())
    .await?;
    let local: Vec<u64> = local
        .into_iter()
        .filter_map(|count| u64::try_from(count).ok())
        .collect();
    let mut estimates = Estimates::new();
    for (field, values) in counts {
        let stated = values.values().copied().fold(0, u64::saturating_add);
        if let Some(u) = estimate::u_of(values.values().copied()) {
            estimates.insert(field, (u, stated));
        }
    }
    let stated = local.iter().copied().fold(0, u64::saturating_add);
    if let Some(u) = estimate::u_of(local.iter().copied()) {
        estimates.insert(Field::LocalIdentifier, (u, stated));
    }
    Ok(estimates)
}
