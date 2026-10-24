// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The cross-reference of one identifier, as a PIX query reads it.
//!
//! The record is found by `(domain, hmac)` of the value its domain's rule
//! normalises; a record the Source subsumed is followed to its survivor
//! (PIXm 3.1.0 §2:3.104.4.2.3: the references to the subsumed identifier are
//! replaced by the survivor's); its open link names the person, followed
//! through `merged_into` to the active survivor; and the answer is every
//! identifier of every open link of that person, each in its domain (PIXm
//! 3.1.0 §2:3.83.4.1.3). A removed identifier is not found (§2:3.104.4.3.3).
//! The read takes no lock (`docs/architecture.md` §4.3).

use sqlx::PgConnection;
use uuid::Uuid;

use crate::store::StoreError;
use crate::store::identifier::Rule;
use crate::store::record::{self, PersonId};
use crate::store::seal::{IdentifierValue, SealError, Sealer};

/// The hops a chain of subsumed records or merged persons is followed.
const MAX_HOPS: i32 = 64;

/// What the store holds for one identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// The domain is unknown or retired (PIXm 3.1.0 §2:3.83.4.2.2.3).
    UnknownDomain,
    /// The domain is known and the identifier is not, or was removed
    /// (§2:3.83.4.2.2.2, §2:3.104.4.3.3).
    NotFound,
    /// The person the identifier is cross-referenced to, and every
    /// identifier the person holds.
    Found(CrossReference),
}

/// The identifiers of one person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossReference {
    /// The person.
    pub person: PersonId,
    /// Every identifier of every open link of the person, each with its
    /// domain's system, ordered by system.
    pub identifiers: Vec<(String, IdentifierValue)>,
}

/// An identifier of the person as stored: domain, system, sealed value,
/// plain value.
type IdentifierRow = (Uuid, String, Option<Vec<u8>>, Option<String>);

/// Reads the cross-reference of `value` in the domain `system` names.
///
/// # Errors
/// Returns [`StoreError::Query`] when a statement fails, and
/// [`StoreError::Seal`] when a sealed value does not open under `keys`.
pub async fn lookup(
    connection: &mut PgConnection,
    keys: &Sealer,
    system: &str,
    value: &str,
) -> Result<Lookup, StoreError> {
    let Some(domain) = record::domain(&mut *connection, system).await? else {
        return Ok(Lookup::UnknownDomain);
    };
    if domain.retired {
        return Ok(Lookup::UnknownDomain);
    }
    // NOTE: PIXm 3.1.0 §2:3.83.4.2.2.2; a value its domain's rule refuses
    // names no record the store can hold, so it is not found.
    let Ok(value) = Rule::of(system).normalise(value) else {
        return Ok(Lookup::NotFound);
    };
    let hmac = keys.hmac(domain.id.uuid(), &value);
    let person: Option<Uuid> = sqlx::query_scalar(
        "WITH RECURSIVE chain (id, replaced_by, removed, hops) AS (
             SELECT id, replaced_by, removed_at IS NOT NULL, 0
             FROM pix.source_record WHERE domain_id = $1 AND identifier_hmac = $2
             UNION ALL
             SELECT r.id, r.replaced_by, r.removed_at IS NOT NULL, c.hops + 1
             FROM pix.source_record r JOIN chain c ON r.id = c.replaced_by
             WHERE c.hops < $3
         ),
         survivor AS (
             SELECT id, removed FROM chain WHERE replaced_by IS NULL
         ),
         people (id, merged_into, hops) AS (
             SELECT p.id, p.merged_into, 0
             FROM survivor s
             JOIN pix.link l ON l.record_id = s.id AND upper_inf(l.valid)
             JOIN pix.person p ON p.id = l.person_id
             WHERE NOT s.removed
             UNION ALL
             SELECT p.id, p.merged_into, q.hops + 1
             FROM pix.person p JOIN people q ON p.id = q.merged_into
             WHERE q.hops < $3
         )
         SELECT id FROM people WHERE merged_into IS NULL",
    )
    .bind(domain.id.uuid())
    .bind(hmac.as_slice())
    .bind(MAX_HOPS)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(person) = person else {
        return Ok(Lookup::NotFound);
    };
    let rows: Vec<IdentifierRow> = sqlx::query_as(
        "SELECT r.domain_id, d.system, r.identifier_sealed, r.identifier_value
         FROM pix.link l
         JOIN pix.source_record r ON r.id = l.record_id
         JOIN pix.identifier_domain d ON d.id = r.domain_id
         WHERE l.person_id = $1 AND upper_inf(l.valid)
           AND r.active AND r.removed_at IS NULL AND d.retired_at IS NULL
         ORDER BY d.system, r.id",
    )
    .bind(person)
    .fetch_all(&mut *connection)
    .await?;
    let mut identifiers = Vec::with_capacity(rows.len());
    for (domain, system, sealed, plain) in rows {
        let value = match (sealed, plain) {
            (Some(sealed), _) => keys.open(domain, &sealed)?,
            (None, Some(plain)) => IdentifierValue::new(plain).ok_or(SealError::Malformed)?,
            (None, None) => return Err(StoreError::Seal(SealError::Malformed)),
        };
        identifiers.push((system, value));
    }
    Ok(Lookup::Found(CrossReference {
        person: PersonId::from_uuid(person),
        identifiers,
    }))
}
