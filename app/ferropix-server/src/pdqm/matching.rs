// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-119 match over the scorer.
//!
//! The input `Patient` is read as if it were a record, the records that
//! share a blocking key with it are scored by [`crate::matching::scorer`],
//! and each person is graded by its best record. The candidates are found by the configured blocking keys of
//! `docs/architecture.md` §5.2, as the feed finds them; the input's
//! identifiers in `local` domains are read by their HMAC, never compared in
//! clear. A person whose best score grades `certainly-not` is no match and
//! is left out. A merged person holds no record, so the match finds its
//! survivor (PDQm 3.2.0 §2:3.119.4.1.3, Case 8, the second option). PDQm
//! leaves the matching to the Supplier (§2:3.119.4.1.3): our own design.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fhir_types::r4::patient::Patient;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::config::section::domain::DomainKind;
use crate::config::section::matching::BlockingKey;
use crate::feed::demographics;
use crate::matching::blocking;
use crate::matching::scorer::{Grade, Scorer};
use crate::matching::subject::{IdentifierKey, Subject};
use crate::pdqm::PdqError;
use crate::store::StoreError;
use crate::store::identifier::Rule;
use crate::store::record;
use crate::store::seal::{SealError, Sealer};

/// The fewest of the scorer's fields an input states before the answer
/// warns that the match rests on too little (PDQm 3.2.0 §2:3.119.4.1.3,
/// Case 10; the number is our own design).
pub const FEW_FIELDS: usize = 3;

/// One person the input matched.
#[derive(Clone, Copy, PartialEq)]
pub struct Found {
    /// The person.
    pub person: Uuid,
    /// The best score of the person's records, in `[0, 1]`.
    pub score: f64,
    /// The grade of that score.
    pub grade: Grade,
}

impl fmt::Debug for Found {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Found")
            .field("score", &self.score)
            .field("grade", &self.grade)
            .finish_non_exhaustive()
    }
}

/// What a match found.
#[derive(Debug, Clone, PartialEq)]
pub struct Matched {
    /// The persons, from the highest score to the lowest, ties by id.
    pub found: Vec<Found>,
    /// Whether the input states fewer than [`FEW_FIELDS`] of the scorer's
    /// fields.
    pub few_fields: bool,
}

/// A candidate record: id, person, demographics.
type CandidateRow = (Uuid, Uuid, String);

/// Matches `input` against the store with `scorer`, over the blocking keys
/// `keys`, with `sealer` giving the HMAC of the input's identifiers.
///
/// # Errors
/// Returns [`PdqError::Store`] when a statement fails and
/// [`PdqError::Demographics`] when stored demographics are no `Patient`.
pub async fn run(
    connection: &mut PgConnection,
    sealer: &Sealer,
    scorer: &Scorer,
    keys: &[BlockingKey],
    input: &Patient,
) -> Result<Matched, PdqError> {
    let local = local_keys(&mut *connection, sealer, input).await?;
    let subject = Subject::from_patient(input, local.clone());
    let few_fields = subject.stated() < FEW_FIELDS;
    let mut candidates: BTreeMap<Uuid, (CandidateRow, BlockingKey)> = BTreeMap::new();
    for key in keys {
        let rows: Vec<CandidateRow> = match key {
            BlockingKey::BirthYearFamilyPhonetic | BlockingKey::PostalCodeBirthDate => {
                let Some(value) = blocking::text_key(*key, &subject) else {
                    continue;
                };
                let column = if *key == BlockingKey::BirthYearFamilyPhonetic {
                    "block_phonetic"
                } else {
                    "block_postal"
                };
                sqlx::query_as(sqlx::AssertSqlSafe(format!(
                    "SELECT r.id, l.person_id, v.demographics::text
                     FROM pix.source_record_version v
                     JOIN pix.source_record r ON r.id = v.record_id AND v.version = r.head_version
                     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
                     JOIN pix.identifier_domain d ON d.id = r.domain_id
                     WHERE v.{column} = $1
                       AND r.active AND r.removed_at IS NULL AND d.retired_at IS NULL"
                )))
                .bind(value)
                .fetch_all(&mut *connection)
                .await
                .map_err(store)?
            }
            BlockingKey::SharedIdentifier => {
                if local.is_empty() {
                    continue;
                }
                let (domains, hmacs): (Vec<Uuid>, Vec<Vec<u8>>) = local
                    .iter()
                    .map(|key| (key.domain(), key.hmac().to_vec()))
                    .unzip();
                sqlx::query_as(
                    "SELECT DISTINCT r.id, l.person_id, v.demographics::text
                     FROM pix.carried_identifier i
                     JOIN pix.source_record r ON r.id = i.record_id AND i.version = r.head_version
                     JOIN pix.source_record_version v
                          ON v.record_id = r.id AND v.version = r.head_version
                     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
                     JOIN pix.identifier_domain d ON d.id = r.domain_id
                     WHERE (i.domain_id, i.identifier_hmac)
                               IN (SELECT * FROM unnest($1::uuid[], $2::bytea[]))
                       AND r.active AND r.removed_at IS NULL AND d.retired_at IS NULL",
                )
                .bind(&domains)
                .bind(&hmacs)
                .fetch_all(&mut *connection)
                .await
                .map_err(store)?
            }
        };
        for row in rows {
            candidates.entry(row.0).or_insert((row, *key));
        }
    }
    let records: Vec<Uuid> = candidates.keys().copied().collect();
    let mut carried = carried_local(&mut *connection, &records).await?;
    let mut best: BTreeMap<Uuid, (f64, Grade)> = BTreeMap::new();
    for (record, ((_, person, text), key)) in candidates {
        let patient = demographics::patient(&text)?;
        let other = Subject::from_patient(&patient, carried.remove(&record).unwrap_or_default());
        let pair = scorer.score(&subject, &other, Some(key));
        let entry = best.entry(person).or_insert((pair.score, pair.grade));
        if pair.score > entry.0 {
            *entry = (pair.score, pair.grade);
        }
    }
    let mut found: Vec<Found> = best
        .into_iter()
        .filter(|(_, (_, grade))| *grade != Grade::CertainlyNot)
        .map(|(person, (score, grade))| Found {
            person,
            score,
            grade,
        })
        .collect();
    found.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then(left.person.cmp(&right.person))
    });
    Ok(Matched { found, few_fields })
}

/// Returns the keys of the input's identifiers in `local` domains the store
/// knows: each value normalised by its domain's rule and read as its HMAC.
async fn local_keys(
    connection: &mut PgConnection,
    sealer: &Sealer,
    input: &Patient,
) -> Result<BTreeSet<IdentifierKey>, PdqError> {
    let domains = record::domains(&mut *connection).await?;
    let mut keys = BTreeSet::new();
    for identifier in &input.identifier {
        let (Some(system), Some(value)) = (
            identifier.system.as_ref().and_then(|s| s.value.as_deref()),
            identifier.value.as_ref().and_then(|v| v.value.as_deref()),
        ) else {
            continue;
        };
        let Some(domain) = domains.iter().find(|domain| {
            domain.system == system && domain.kind == DomainKind::Local && !domain.retired
        }) else {
            continue;
        };
        // NOTE: PDQm 3.2.0 §2:3.119.4.1.3, the matching is the Supplier's; a
        // value its domain's rule refuses is no identifier to compare.
        if let Ok(normalised) = Rule::of(system).normalise(value) {
            keys.insert(IdentifierKey::new(
                domain.id.uuid(),
                sealer.hmac(domain.id.uuid(), &normalised),
            ));
        }
    }
    Ok(keys)
}

/// Returns, by record, the keys of the identifiers each record's current
/// version carries in `local` domains.
async fn carried_local(
    connection: &mut PgConnection,
    records: &[Uuid],
) -> Result<BTreeMap<Uuid, BTreeSet<IdentifierKey>>, PdqError> {
    let rows: Vec<(Uuid, Uuid, Vec<u8>)> = sqlx::query_as(
        "SELECT i.record_id, i.domain_id, i.identifier_hmac
         FROM pix.carried_identifier i
         JOIN pix.source_record r ON r.id = i.record_id AND i.version = r.head_version
         JOIN pix.identifier_domain d ON d.id = i.domain_id
         WHERE i.record_id = ANY($1) AND d.kind = 'local'",
    )
    .bind(records)
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    let mut keys: BTreeMap<Uuid, BTreeSet<IdentifierKey>> = BTreeMap::new();
    for (record, domain, hmac) in rows {
        let hmac: [u8; 32] = hmac
            .try_into()
            .map_err(|_short: Vec<u8>| StoreError::Seal(SealError::Malformed))?;
        keys.entry(record)
            .or_default()
            .insert(IdentifierKey::new(domain, hmac));
    }
    Ok(keys)
}

/// Wraps a driver error.
fn store(source: sqlx::Error) -> PdqError {
    PdqError::Store(source.into())
}
