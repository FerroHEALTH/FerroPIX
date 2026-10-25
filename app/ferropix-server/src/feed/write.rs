// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The rows a feed writes inside its transaction: the record locked or
//! inserted, its version, the identifiers the version carries, the
//! advisory locks, and the steward action of each system action
//! (`docs/architecture.md` §4.3). No specification governs the writes: our
//! own design.

use fhir_types::r4::patient::Patient;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::config::section::domain::DomainKind;
use crate::feed::{Context, FeedError};
use crate::store::StoreError;
use crate::store::identifier::Rule;
use crate::store::record::{Demographics, Domain, DomainId, PersonId, RecordId};
use crate::store::seal::{IdentifierValue, Sealer};

/// One identifier a version carries, in a domain the store knows.
#[derive(Debug, Clone)]
pub(super) struct Carried {
    /// The domain.
    pub domain: DomainId,
    /// What the domain's identifiers are.
    pub kind: DomainKind,
    /// Whether the domain seals its values.
    pub sensitive: bool,
    /// The value, as the domain's rule normalised it.
    pub value: IdentifierValue,
    /// The HMAC of the domain id and the value.
    pub hmac: [u8; 32],
}

/// Returns the identifiers `patient` carries in the domains of `domains`
/// that are not retired, each normalised by its domain's rule, ordered and
/// without repeats.
///
/// An identifier of a domain the store does not know names nothing the
/// cross-reference holds and is left out.
pub(super) fn carried(
    patient: &Patient,
    domains: &[Domain],
    keys: &Sealer,
) -> Result<Vec<Carried>, FeedError> {
    let mut carried: Vec<Carried> = Vec::new();
    for identifier in &patient.identifier {
        let system = identifier.system.as_ref().and_then(|s| s.value.as_deref());
        let value = identifier.value.as_ref().and_then(|v| v.value.as_deref());
        let (Some(system), Some(value)) = (system, value) else {
            continue;
        };
        // NOTE: no specification governs this: our own design; an unknown or
        // retired domain is no assigning authority the cross-reference keeps.
        let Some(domain) = domains.iter().find(|d| d.system == system && !d.retired) else {
            continue;
        };
        let value = Rule::of(system)
            .normalise(value)
            .map_err(|source| FeedError::Identifier {
                system: system.to_owned(),
                source,
            })?;
        let hmac = keys.hmac(domain.id.uuid(), &value);
        carried.push(Carried {
            domain: domain.id,
            kind: domain.kind,
            sensitive: domain.sensitive,
            value,
            hmac,
        });
    }
    carried.sort_by_key(|c| (c.domain, c.hmac));
    carried.dedup_by(|a, b| a.domain == b.domain && a.hmac == b.hmac);
    Ok(carried)
}

/// Returns the advisory lock key of the identifier whose HMAC is `hmac`:
/// its first eight bytes, which the HMAC's domain id already scopes.
pub(super) fn identity_key(hmac: &[u8; 32]) -> i64 {
    let [b0, b1, b2, b3, b4, b5, b6, b7, ..] = *hmac;
    i64::from_be_bytes([b0, b1, b2, b3, b4, b5, b6, b7])
}

/// Returns the advisory lock key of `person`: the last eight bytes of its
/// uuid, the random part of a version 7 uuid.
pub(super) fn person_key(person: PersonId) -> i64 {
    let [_, _, _, _, _, _, _, _, tail @ ..] = *person.uuid().as_bytes();
    i64::from_be_bytes(tail)
}

/// Takes the transaction's advisory lock on each of `keys`, in ascending
/// order, so two transactions that need the same keys never deadlock.
pub(super) async fn advisory(
    connection: &mut PgConnection,
    mut keys: Vec<i64>,
) -> Result<(), StoreError> {
    keys.sort_unstable();
    keys.dedup();
    for key in keys {
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(key)
            .execute(&mut *connection)
            .await?;
    }
    Ok(())
}

/// A record locked for the transaction.
#[derive(Debug, Clone, Copy)]
pub(super) struct Locked {
    /// The record.
    pub id: RecordId,
    /// Whether this transaction inserted it.
    pub created: bool,
    /// Its head version before this transaction, `0` when inserted.
    pub head: i32,
    /// Whether it is active: not subsumed by the Source.
    pub active: bool,
    /// Whether it was removed.
    pub removed: bool,
}

/// Locks the record `hmac` names in `domain` with `SELECT … FOR UPDATE`.
pub(super) async fn lock(
    connection: &mut PgConnection,
    domain: DomainId,
    hmac: &[u8; 32],
) -> Result<Option<Locked>, StoreError> {
    let row: Option<(Uuid, i32, bool, bool)> = sqlx::query_as(
        "SELECT id, head_version, active, removed_at IS NOT NULL
         FROM pix.source_record WHERE domain_id = $1 AND identifier_hmac = $2
         FOR UPDATE",
    )
    .bind(domain.uuid())
    .bind(hmac.as_slice())
    .fetch_optional(&mut *connection)
    .await?;
    Ok(row.map(|(id, head, active, removed)| Locked {
        id: RecordId::from_uuid(id),
        created: false,
        head,
        active,
        removed,
    }))
}

/// Locks the record of `value` in `domain`, or inserts it.
///
/// An inserted record waits for its first version, which the deferred
/// head-version constraint requires before the commit.
pub(super) async fn lock_or_insert(
    connection: &mut PgConnection,
    keys: &Sealer,
    domain: &Domain,
    value: &IdentifierValue,
) -> Result<Locked, StoreError> {
    let domain_id = domain.id.uuid();
    let hmac = keys.hmac(domain_id, value);
    if let Some(locked) = lock(connection, domain.id, &hmac).await? {
        return Ok(locked);
    }
    let (sealed, plain) = if domain.sensitive {
        (Some(keys.seal(domain_id, value)?), None)
    } else {
        (None, Some(value.expose()))
    };
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO pix.source_record
             (domain_id, identifier_hmac, identifier_sealed, identifier_value, key_id)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id",
    )
    .bind(domain_id)
    .bind(hmac.as_slice())
    .bind(sealed)
    .bind(plain)
    .bind(keys.key_id().as_bytes().as_slice())
    .fetch_one(&mut *connection)
    .await?;
    Ok(Locked {
        id: RecordId::from_uuid(id),
        created: true,
        head: 0,
        active: true,
        removed: false,
    })
}

/// The text blocking keys of a version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Blocks {
    /// `(birth year, phonetic key of the family name)`.
    pub phonetic: Option<String>,
    /// `(postal code, birth date)`.
    pub postal: Option<String>,
}

/// Inserts the next version of `record` and makes it the head, a removed
/// record coming back with it; returns the version's number.
pub(super) async fn add_version(
    connection: &mut PgConnection,
    record: &Locked,
    demographics: &Demographics,
    blocks: &Blocks,
    context: &Context,
) -> Result<i32, StoreError> {
    let version = record
        .head
        .checked_add(1)
        .ok_or(StoreError::VersionOverflow)?;
    sqlx::query(
        "INSERT INTO pix.source_record_version
             (record_id, version, demographics, channel, request_id, block_phonetic, block_postal)
         VALUES ($1, $2, $3::jsonb, $4, $5, $6, $7)",
    )
    .bind(record.id.uuid())
    .bind(version)
    .bind(demographics.expose())
    .bind(context.channel.as_str())
    .bind(context.request_id.as_deref())
    .bind(blocks.phonetic.as_deref())
    .bind(blocks.postal.as_deref())
    .execute(&mut *connection)
    .await?;
    if !record.created {
        sqlx::query(
            "UPDATE pix.source_record SET head_version = $2, removed_at = NULL WHERE id = $1",
        )
        .bind(record.id.uuid())
        .bind(version)
        .execute(&mut *connection)
        .await?;
    }
    Ok(version)
}

/// Inserts the identifiers version `version` of `record` carries, each
/// sealed in a sensitive domain and plain in another.
pub(super) async fn insert_carried(
    connection: &mut PgConnection,
    keys: &Sealer,
    record: RecordId,
    version: i32,
    carried: &[Carried],
) -> Result<(), StoreError> {
    for identifier in carried {
        let domain = identifier.domain.uuid();
        let (sealed, plain) = if identifier.sensitive {
            (Some(keys.seal(domain, &identifier.value)?), None)
        } else {
            (None, Some(identifier.value.expose()))
        };
        sqlx::query(
            "INSERT INTO pix.carried_identifier
                 (record_id, version, domain_id, identifier_hmac, identifier_sealed,
                  identifier_value, key_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(record.uuid())
        .bind(version)
        .bind(domain)
        .bind(identifier.hmac.as_slice())
        .bind(sealed)
        .bind(plain)
        .bind(keys.key_id().as_bytes().as_slice())
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

/// What a steward action row names.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Subjects {
    /// The person the action concerns.
    pub person: Option<PersonId>,
    /// The record the action concerns.
    pub record: Option<RecordId>,
    /// The candidate the action recorded.
    pub candidate: Option<Uuid>,
    /// The merge the action made.
    pub merge: Option<Uuid>,
}

/// Inserts the steward action row of one system action of kind `kind` and
/// returns its id.
pub(super) async fn action(
    connection: &mut PgConnection,
    kind: &str,
    context: &Context,
    subjects: Subjects,
) -> Result<Uuid, StoreError> {
    reasoned(connection, kind, context, subjects, None).await
}

/// Inserts the steward action row of kind `kind`, with the `reason` a
/// steward gave, and returns its id.
pub(super) async fn reasoned(
    connection: &mut PgConnection,
    kind: &str,
    context: &Context,
    subjects: Subjects,
    reason: Option<&str>,
) -> Result<Uuid, StoreError> {
    let id = sqlx::query_scalar(
        "INSERT INTO pix.steward_action
             (kind, actor, person_id, record_id, candidate_id, merge_id, request_id, reason)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id",
    )
    .bind(kind)
    .bind(&context.actor)
    .bind(subjects.person.map(PersonId::uuid))
    .bind(subjects.record.map(RecordId::uuid))
    .bind(subjects.candidate)
    .bind(subjects.merge)
    .bind(context.request_id.as_deref())
    .bind(reason)
    .fetch_one(&mut *connection)
    .await?;
    Ok(id)
}
