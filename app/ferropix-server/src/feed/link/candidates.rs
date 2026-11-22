// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The other records a version is weighed against: those carrying one of
//! its authoritative identifiers, and those sharing a blocking key with
//! it in another person.

use std::collections::BTreeMap;

use sqlx::PgConnection;
use uuid::Uuid;

use crate::config::section::matching::BlockingKey;
use crate::feed::{FeedError, Feeder};
use crate::matching::features;
use crate::matching::subject::IdentifierKey;
use crate::store::blocking::{
    FEED_BY_IDENTIFIER, FEED_BY_PHONETIC, FEED_BY_POSTAL, FEED_BY_SHARED, carried_keys,
};
use crate::store::record::{PersonId, RecordId};

use super::{Me, Other, store};

/// A row of another record: id, current version, person, scoring features
/// and their version, and its local identifiers' keys.
pub(super) type OtherRow = (
    Uuid,
    i32,
    Uuid,
    Option<String>,
    Option<i16>,
    Option<Vec<u8>>,
);

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
    let rows: Vec<OtherRow> = sqlx::query_as(FEED_BY_IDENTIFIER)
        .bind(&domains)
        .bind(&hmacs)
        .bind(me.record.uuid())
        .fetch_all(&mut *connection)
        .await
        .map_err(store)?;
    others(rows)
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
                    (FEED_BY_PHONETIC, me.phonetic.as_deref())
                } else {
                    (FEED_BY_POSTAL, me.postal.as_deref())
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
                sqlx::query_as(FEED_BY_SHARED)
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
    others(rows.into_values().collect())
}

/// Reads each row's scoring features and the identifiers its version
/// carries in `local` domains into an [`Other`].
pub(super) fn others(rows: Vec<OtherRow>) -> Result<Vec<Other>, FeedError> {
    rows.into_iter()
        .map(|(record, version, person, stored, stored_version, keys)| {
            let local = carried_keys(keys.as_deref())
                .map_err(|error| FeedError::Store(crate::store::StoreError::Seal(error)))?;
            let subject = features::decode(stored.as_deref(), stored_version, local)
                .map_err(|error| FeedError::Store(error.into()))?;
            Ok(Other {
                record: RecordId::from_uuid(record),
                version,
                person: PersonId::from_uuid(person),
                subject,
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
