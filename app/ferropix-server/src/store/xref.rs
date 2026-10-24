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
//!
//! An identifier in an `authoritative` domain that no record of its own
//! holds, such as a BSN the Sources' Patients carry and no Source feeds, is
//! found through the current versions that carry it, and is answered among
//! the person's identifiers (#167). Carried in records of two persons, it is
//! [`Lookup::Ambiguous`]. No specification governs these rules: our own
//! design.

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
    /// The identifier has no record of its own and the records that carry
    /// it are in more than one person, which the steward has to resolve.
    Ambiguous,
}

/// The identifiers of one person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossReference {
    /// The person.
    pub person: PersonId,
    /// Every identifier of every open link of the person, and every
    /// authoritative identifier those records carry that has no record of
    /// its own, each with its domain's system, ordered by system.
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
    let persons: Vec<Uuid> = sqlx::query_scalar(
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
         start (id) AS (
             SELECT l.person_id
             FROM survivor s
             JOIN pix.link l ON l.record_id = s.id AND upper_inf(l.valid)
             WHERE NOT s.removed
             UNION
             SELECT l.person_id
             FROM pix.carried_identifier c
             JOIN pix.identifier_domain d ON d.id = c.domain_id
             JOIN pix.source_record r ON r.id = c.record_id AND r.head_version = c.version
             JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
             WHERE c.domain_id = $1 AND c.identifier_hmac = $2
               AND d.kind = 'authoritative'
               AND r.active AND r.removed_at IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM pix.source_record o
                   WHERE o.domain_id = $1 AND o.identifier_hmac = $2
               )
         ),
         people (id, merged_into, hops) AS (
             SELECT p.id, p.merged_into, 0
             FROM start s JOIN pix.person p ON p.id = s.id
             UNION ALL
             SELECT p.id, p.merged_into, q.hops + 1
             FROM pix.person p JOIN people q ON p.id = q.merged_into
             WHERE q.hops < $3
         )
         SELECT DISTINCT id FROM people WHERE merged_into IS NULL ORDER BY id",
    )
    .bind(domain.id.uuid())
    .bind(hmac.as_slice())
    .bind(MAX_HOPS)
    .fetch_all(&mut *connection)
    .await?;
    let person = match persons.as_slice() {
        [] => return Ok(Lookup::NotFound),
        [person] => *person,
        _ => return Ok(Lookup::Ambiguous),
    };
    let identifiers = identifiers(connection, keys, person).await?;
    Ok(Lookup::Found(CrossReference {
        person: PersonId::from_uuid(person),
        identifiers,
    }))
}

/// Reads every identifier of `person`: its records' own and the
/// authoritative ones they carry with no record of their own, ordered by
/// system.
///
/// The PDQm Supplier serves the same identifiers in each `Patient`, so a
/// person answers alike in ITI-83 and ITI-78.
///
/// # Errors
/// Returns [`StoreError::Query`] when a statement fails, and
/// [`StoreError::Seal`] when a sealed value does not open under `keys`.
pub async fn identifiers(
    connection: &mut PgConnection,
    keys: &Sealer,
    person: Uuid,
) -> Result<Vec<(String, IdentifierValue)>, StoreError> {
    let mut rows: Vec<IdentifierRow> = sqlx::query_as(
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
    let carried: Vec<IdentifierRow> = sqlx::query_as(
        "SELECT DISTINCT ON (d.system, c.identifier_hmac)
                c.domain_id, d.system, c.identifier_sealed, c.identifier_value
         FROM pix.link l
         JOIN pix.source_record r ON r.id = l.record_id
         JOIN pix.carried_identifier c ON c.record_id = r.id AND c.version = r.head_version
         JOIN pix.identifier_domain d ON d.id = c.domain_id
         WHERE l.person_id = $1 AND upper_inf(l.valid)
           AND r.active AND r.removed_at IS NULL
           AND d.kind = 'authoritative' AND d.retired_at IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM pix.source_record o
               WHERE o.domain_id = c.domain_id AND o.identifier_hmac = c.identifier_hmac
           )
         ORDER BY d.system, c.identifier_hmac",
    )
    .bind(person)
    .fetch_all(&mut *connection)
    .await?;
    rows.extend(carried);
    rows.sort_by(|left, right| left.1.cmp(&right.1));
    let mut identifiers = Vec::with_capacity(rows.len());
    for (domain, system, sealed, plain) in rows {
        let value = match (sealed, plain) {
            (Some(sealed), _) => keys.open(domain, &sealed)?,
            (None, Some(plain)) => IdentifierValue::new(plain).ok_or(SealError::Malformed)?,
            (None, None) => return Err(StoreError::Seal(SealError::Malformed)),
        };
        identifiers.push((system, value));
    }
    Ok(identifiers)
}
