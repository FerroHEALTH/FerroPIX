// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What one attempt of a feed reads before it writes: the record's current
//! state, the condition it is held to, the identity and local keys it
//! blocks on, and the person changes it announces. No specification governs
//! this split: our own design.

use crate::config::section::domain::DomainKind;
use crate::config::section::matching::BlockingKey;
use crate::feed::link::Me;
use crate::feed::write::Carried;
use crate::feed::{FeedError, announce, demographics, master, store, write};
use crate::matching::blocking;
use crate::matching::subject::{IdentifierKey, Subject};
use crate::store::StoreError;
use crate::store::identifier::Rule;
use crate::store::record::{Domain, PersonId, RecordId};
use sqlx::PgConnection;
use std::collections::BTreeSet;
use uuid::Uuid;

/// The changes a feed made to persons: each merged person, then the person
/// its record is in, created when `created` and updated otherwise.
pub(super) fn announced(
    person: Option<PersonId>,
    merged: &[PersonId],
    created: bool,
) -> Vec<announce::Change> {
    let mut changes: Vec<announce::Change> = merged
        .iter()
        .copied()
        .map(announce::Change::Merged)
        .collect();
    changes.extend(person.map(|person| {
        if created {
            announce::Change::Created(person)
        } else {
            announce::Change::Updated(person)
        }
    }));
    changes
}

/// Which record a revision writes.
#[derive(Debug, Clone, Copy)]
pub(super) struct Revising<'a> {
    /// The domain's system.
    pub(super) system: &'a str,
    /// The identifier's value, as sent.
    pub(super) value: &'a str,
    /// The master identity an ITI-93 change writes, `None` for a feed.
    pub(super) master: Option<master::Identity>,
}

/// Which record a resolved duplicate subsumes, and into which.
#[derive(Debug, Clone, Copy)]
pub(super) struct Resolving<'a> {
    /// The domain of both records.
    pub(super) system: &'a str,
    /// The subsumed identifier's value.
    pub(super) subsumed: &'a str,
    /// The surviving identifier's value.
    pub(super) survivor: &'a str,
}

/// Returns the domain `system` names and the normalised `value`.
pub(super) fn condition<'d>(
    domains: &'d [Domain],
    system: &str,
    value: &str,
) -> Result<(&'d Domain, crate::store::seal::IdentifierValue), FeedError> {
    let domain = domains
        .iter()
        .find(|d| d.system == system && !d.retired)
        .ok_or(FeedError::UnknownDomain)?;
    let value = Rule::of(system)
        .normalise(value)
        .map_err(|source| FeedError::Identifier {
            system: system.to_owned(),
            source,
        })?;
    Ok((domain, value))
}

/// Returns the advisory keys of the record's own identifier and of every
/// authoritative identifier it carries.
pub(super) fn identity_keys(own: &[u8; 32], carried: &[Carried]) -> Vec<i64> {
    std::iter::once(write::identity_key(own))
        .chain(
            carried
                .iter()
                .filter(|c| c.kind == DomainKind::Authoritative)
                .map(|c| write::identity_key(&c.hmac)),
        )
        .collect()
}

/// Returns the keys of the identifiers `carried` holds in `local` domains.
pub(super) fn local_keys(carried: &[Carried]) -> BTreeSet<IdentifierKey> {
    carried
        .iter()
        .filter(|c| c.kind == DomainKind::Local)
        .map(|c| IdentifierKey::new(c.domain.uuid(), c.hmac))
        .collect()
}

/// Returns the version the cross-referencing runs over, of a record in the
/// master domain when `master`.
pub(super) fn me<'s>(
    record: RecordId,
    version: i32,
    subject: &'s Subject,
    carried: &[Carried],
    master: bool,
) -> Me<'s> {
    let keys = |kind: DomainKind| {
        carried
            .iter()
            .filter(|c| c.kind == kind)
            .map(|c| IdentifierKey::new(c.domain.uuid(), c.hmac))
            .collect()
    };
    Me {
        record,
        version,
        master,
        identity: None,
        subject,
        authoritative: keys(DomainKind::Authoritative),
        local: keys(DomainKind::Local),
        phonetic: blocking::text_key(BlockingKey::BirthYearFamilyPhonetic, subject),
        postal: blocking::text_key(BlockingKey::PostalCodeBirthDate, subject),
    }
}

/// A carried identifier as stored: domain, HMAC, sealed value, plain value.
type CarriedRow = (Uuid, Vec<u8>, Option<Vec<u8>>, Option<String>);

/// Reads the current version of `record`: its subject, the identifiers it
/// carries, opened, and its number.
pub(super) async fn current(
    connection: &mut PgConnection,
    keys: &crate::store::seal::Sealer,
    record: RecordId,
    domains: &[Domain],
) -> Result<(Subject, Vec<Carried>, i32), FeedError> {
    let (version, text): (i32, String) = sqlx::query_as(
        "SELECT v.version, v.demographics::text
         FROM pix.source_record r
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
         WHERE r.id = $1",
    )
    .bind(record.uuid())
    .fetch_one(&mut *connection)
    .await
    .map_err(store)?;
    let rows: Vec<CarriedRow> = sqlx::query_as(
        "SELECT domain_id, identifier_hmac, identifier_sealed, identifier_value
         FROM pix.carried_identifier WHERE record_id = $1 AND version = $2
         ORDER BY domain_id, identifier_hmac",
    )
    .bind(record.uuid())
    .bind(version)
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    let malformed = || FeedError::Store(StoreError::Seal(crate::store::seal::SealError::Malformed));
    let mut carried = Vec::with_capacity(rows.len());
    for (domain_id, hmac, sealed, plain) in rows {
        let Some(domain) = domains.iter().find(|d| d.id.uuid() == domain_id) else {
            return Err(malformed());
        };
        let value = match (sealed, plain) {
            (Some(sealed), _) => keys.open(domain_id, &sealed).map_err(StoreError::Seal)?,
            (None, Some(plain)) => {
                crate::store::seal::IdentifierValue::new(plain).ok_or_else(malformed)?
            }
            (None, None) => return Err(malformed()),
        };
        carried.push(Carried {
            domain: domain.id,
            kind: domain.kind,
            sensitive: domain.sensitive,
            value,
            hmac: hmac.try_into().map_err(|_short| malformed())?,
        });
    }
    let patient = demographics::patient(&text)?;
    Ok((
        Subject::from_patient(&patient, local_keys(&carried)),
        carried,
        version,
    ))
}
