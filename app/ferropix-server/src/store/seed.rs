// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[[domain]]` seed, written as insert-if-absent.
//!
//! A seeded domain absent from the store is inserted; a present one is left
//! as the store holds it, since the store is the record and a steward may
//! have changed it, and a difference from the seed is logged by the domain's
//! system. Each domain names its one Patient Identity Source (PIXm 3.1.0
//! §2:3.104.4.1.3). No specification governs the seed: our own design.

use crate::config::section::domain::{DomainKind, DomainSeed};
use crate::store::{Store, StoreError};

/// What the seed did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeedReport {
    /// The domains inserted.
    pub inserted: usize,
    /// The domains the store held already, left as they were.
    pub kept: usize,
}

/// Returns the stored spelling of a domain kind.
#[must_use]
pub const fn kind_name(kind: DomainKind) -> &'static str {
    match kind {
        DomainKind::Local => "local",
        DomainKind::Authoritative => "authoritative",
        DomainKind::Master => "master",
    }
}

/// Inserts every seeded domain the store lacks, in one transaction.
///
/// # Errors
/// Returns [`StoreError::Query`] when a statement fails, the transaction
/// rolled back.
pub async fn apply(store: &Store, seeds: &[DomainSeed]) -> Result<SeedReport, StoreError> {
    let mut transaction = store.pool().begin().await?;
    let mut report = SeedReport::default();
    for seed in seeds {
        let inserted = sqlx::query(
            "INSERT INTO pix.identifier_domain (system, name, kind, source_client_id, sensitive)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (system) DO NOTHING",
        )
        .bind(&seed.system)
        .bind(&seed.name)
        .bind(kind_name(seed.kind))
        .bind(&seed.source_client_id)
        .bind(seed.sensitive)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if inserted == 1 {
            report.inserted = report.inserted.saturating_add(1);
            tracing::info!(system = seed.system.as_str(), "the domain is seeded");
            continue;
        }
        report.kept = report.kept.saturating_add(1);
        let same: bool = sqlx::query_scalar(
            "SELECT name = $2 AND kind = $3 AND source_client_id = $4 AND sensitive = $5
             FROM pix.identifier_domain WHERE system = $1",
        )
        .bind(&seed.system)
        .bind(&seed.name)
        .bind(kind_name(seed.kind))
        .bind(&seed.source_client_id)
        .bind(seed.sensitive)
        .fetch_one(&mut *transaction)
        .await?;
        if !same {
            tracing::warn!(
                system = seed.system.as_str(),
                "the stored domain differs from its [[domain]] seed; the store is the record and is kept"
            );
        }
    }
    transaction.commit().await?;
    tracing::info!(
        inserted = report.inserted,
        kept = report.kept,
        "the domain seed is applied"
    );
    Ok(report)
}

/// Reads every domain the store holds and has not retired, by system: the
/// record the seed defers to, which names each domain's Source.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails, and
/// [`StoreError::UnknownDomainKind`] for a kind the schema does not admit.
pub async fn stored(store: &Store) -> Result<Vec<DomainSeed>, StoreError> {
    let rows: Vec<(String, String, String, String, bool)> = sqlx::query_as(
        "SELECT system, name, kind, source_client_id, sensitive
         FROM pix.identifier_domain WHERE retired_at IS NULL ORDER BY system",
    )
    .fetch_all(store.pool())
    .await?;
    rows.into_iter()
        .map(|(system, name, kind, source_client_id, sensitive)| {
            let kind = match kind.as_str() {
                "local" => DomainKind::Local,
                "authoritative" => DomainKind::Authoritative,
                "master" => DomainKind::Master,
                _ => return Err(StoreError::UnknownDomainKind { system }),
            };
            Ok(DomainSeed {
                system,
                name,
                kind,
                source_client_id,
                sensitive,
            })
        })
        .collect()
}
