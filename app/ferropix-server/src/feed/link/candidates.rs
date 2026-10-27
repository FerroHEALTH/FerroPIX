// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The other records a version is weighed against: those carrying one of
//! its authoritative identifiers, and those sharing a blocking key with
//! it in another person.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgConnection;
use uuid::Uuid;

use crate::config::section::domain::DomainKind;
use crate::config::section::matching::BlockingKey;
use crate::feed::{FeedError, Feeder, demographics};
use crate::matching::subject::{IdentifierKey, Subject};
use crate::store::record::{PersonId, RecordId};

use super::{Me, Other, store};

/// The records in another person whose current version has the phonetic
/// blocking key `$1`.
pub(super) const BY_PHONETIC: &str =
    "SELECT r.id, r.head_version, l.person_id, v.demographics::text
     FROM pix.source_record_version v
     JOIN pix.source_record r ON r.id = v.record_id AND v.version = r.head_version
     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
     WHERE v.block_phonetic = $1 AND r.id <> $2 AND l.person_id <> $3
       AND r.active AND r.removed_at IS NULL";

/// The records in another person whose current version has the postal
/// blocking key `$1`.
pub(super) const BY_POSTAL: &str = "SELECT r.id, r.head_version, l.person_id, v.demographics::text
     FROM pix.source_record_version v
     JOIN pix.source_record r ON r.id = v.record_id AND v.version = r.head_version
     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
     WHERE v.block_postal = $1 AND r.id <> $2 AND l.person_id <> $3
       AND r.active AND r.removed_at IS NULL";

/// A row of another record: id, current version, person, demographics.
pub(super) type OtherRow = (Uuid, i32, Uuid, String);

/// Returns the records whose current version carries one of `me`'s
/// authoritative identifiers.
pub(super) async fn deterministic(
    connection: &mut PgConnection,
    me: &Me<'_>,
) -> Result<Vec<Other>, FeedError> {
    if me.authoritative.is_empty() {
        return Ok(Vec::new());
    }
    let (domains, hmacs) = split(&me.authoritative);
    let rows: Vec<OtherRow> = sqlx::query_as(
        "SELECT DISTINCT r.id, r.head_version, l.person_id, v.demographics::text
         FROM pix.carried_identifier i
         JOIN pix.source_record r ON r.id = i.record_id AND i.version = r.head_version
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
         JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
         WHERE (i.domain_id, i.identifier_hmac) IN (SELECT * FROM unnest($1::uuid[], $2::bytea[]))
           AND r.id <> $3 AND r.active AND r.removed_at IS NULL
         ORDER BY r.id",
    )
    .bind(&domains)
    .bind(&hmacs)
    .bind(me.record.uuid())
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    others(connection, rows).await
}

/// Returns the records in another person than `survivor` that share one of
/// the configured blocking keys with `me`, each once, ordered by id.
pub(super) async fn blocked(
    connection: &mut PgConnection,
    feeder: &Feeder,
    me: &Me<'_>,
    survivor: PersonId,
) -> Result<Vec<Other>, FeedError> {
    let mut rows: BTreeMap<Uuid, OtherRow> = BTreeMap::new();
    for key in &feeder.blocking {
        let found: Vec<OtherRow> = match key {
            BlockingKey::BirthYearFamilyPhonetic | BlockingKey::PostalCodeBirthDate => {
                let (statement, value) = if *key == BlockingKey::BirthYearFamilyPhonetic {
                    (BY_PHONETIC, me.phonetic.as_deref())
                } else {
                    (BY_POSTAL, me.postal.as_deref())
                };
                let Some(value) = value else {
                    continue;
                };
                sqlx::query_as(statement)
                    .bind(value)
                    .bind(me.record.uuid())
                    .bind(survivor.uuid())
                    .fetch_all(&mut *connection)
                    .await
                    .map_err(store)?
            }
            BlockingKey::SharedIdentifier => {
                if me.local.is_empty() {
                    continue;
                }
                let (domains, hmacs) = split(&me.local);
                sqlx::query_as(
                    "SELECT DISTINCT r.id, r.head_version, l.person_id, v.demographics::text
                     FROM pix.carried_identifier i
                     JOIN pix.source_record r ON r.id = i.record_id AND i.version = r.head_version
                     JOIN pix.source_record_version v
                          ON v.record_id = r.id AND v.version = r.head_version
                     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
                     WHERE (i.domain_id, i.identifier_hmac)
                               IN (SELECT * FROM unnest($1::uuid[], $2::bytea[]))
                       AND r.id <> $3 AND l.person_id <> $4
                       AND r.active AND r.removed_at IS NULL",
                )
                .bind(&domains)
                .bind(&hmacs)
                .bind(me.record.uuid())
                .bind(survivor.uuid())
                .fetch_all(&mut *connection)
                .await
                .map_err(store)?
            }
        };
        for row in found {
            rows.entry(row.0).or_insert(row);
        }
    }
    others(connection, rows.into_values().collect()).await
}

/// Reads each row's demographics and the identifiers its version carries in
/// `local` domains into an [`Other`].
pub(super) async fn others(
    connection: &mut PgConnection,
    rows: Vec<OtherRow>,
) -> Result<Vec<Other>, FeedError> {
    let ids: Vec<Uuid> = rows.iter().map(|row| row.0).collect();
    let local: Vec<(Uuid, Uuid, Vec<u8>)> = sqlx::query_as(
        "SELECT i.record_id, i.domain_id, i.identifier_hmac
         FROM pix.carried_identifier i
         JOIN pix.source_record r ON r.id = i.record_id AND i.version = r.head_version
         JOIN pix.identifier_domain d ON d.id = i.domain_id
         WHERE i.record_id = ANY($1) AND d.kind = $2",
    )
    .bind(&ids)
    .bind(crate::store::seed::kind_name(DomainKind::Local))
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    let mut keys: BTreeMap<Uuid, BTreeSet<IdentifierKey>> = BTreeMap::new();
    for (record, domain, hmac) in local {
        let hmac: [u8; 32] = hmac.try_into().map_err(|_short| {
            FeedError::Store(crate::store::StoreError::Seal(
                crate::store::seal::SealError::Malformed,
            ))
        })?;
        keys.entry(record)
            .or_default()
            .insert(IdentifierKey::new(domain, hmac));
    }
    rows.into_iter()
        .map(|(record, version, person, text)| {
            let patient = demographics::patient(&text)?;
            let local = keys.remove(&record).unwrap_or_default();
            Ok(Other {
                record: RecordId::from_uuid(record),
                version,
                person: PersonId::from_uuid(person),
                subject: Subject::from_patient(&patient, local),
            })
        })
        .collect()
}

/// Splits `keys` into the domain and HMAC arrays a statement binds.
pub(super) fn split(keys: &[IdentifierKey]) -> (Vec<Uuid>, Vec<Vec<u8>>) {
    keys.iter()
        .map(|key| (key.domain(), key.hmac().to_vec()))
        .unzip()
}
