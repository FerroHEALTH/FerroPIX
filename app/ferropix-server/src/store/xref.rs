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
//!
//! [`query`] reads all of it in one statement under one snapshot, with the
//! target domains a query names and the person's pseudonyms (#715). The
//! HMAC is keyed by the domain's id, so the id is known before the statement
//! is sent: [`DomainIds`] keeps each system's id once read, which holds
//! because a domain's system never changes and a domain is never deleted.
//! The statement reads the domain row by that id, so whether the domain is
//! retired is read from the store on every query.

use std::collections::BTreeMap;
use std::sync::{PoisonError, RwLock};

use sqlx::PgConnection;
use uuid::Uuid;

use crate::store::identifier::Rule;
use crate::store::pseudonym::PseudonymDomain;
use crate::store::record::{self, DomainId, PersonId};
use crate::store::seal::{IdentifierValue, SealError, Sealer};
use crate::store::{Store, StoreError};

/// The hops a chain of subsumed records or merged persons is followed.
const MAX_HOPS: i32 = 64;

/// The persons an identifier is cross-referenced to, as the CTEs ending in
/// `found`, over `$1` (the domain), `$2` (the HMAC) and `$3` (the hops).
macro_rules! persons_ctes {
    () => {
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
         ),
         found (id) AS (
             SELECT DISTINCT id FROM people WHERE merged_into IS NULL
         )"
    };
}

/// The identifiers of the person in the CTE `pick`, as the CTE `identified`:
/// its records' own (`part` 0, by record id) and the authoritative ones they
/// carry with no record of their own (`part` 1, by HMAC).
macro_rules! identified_cte {
    () => {
        "identified (domain_id, system, sealed, plain, part, rid, hmac) AS (
             SELECT r.domain_id, d.system, r.identifier_sealed, r.identifier_value,
                    0, r.id, NULL::bytea
             FROM pix.link l
             JOIN pix.source_record r ON r.id = l.record_id
             JOIN pix.identifier_domain d ON d.id = r.domain_id
             WHERE l.person_id = (SELECT id FROM pick) AND upper_inf(l.valid)
               AND r.active AND r.removed_at IS NULL AND d.retired_at IS NULL
             UNION ALL
             SELECT * FROM (
                 SELECT DISTINCT ON (d.system, c.identifier_hmac)
                        c.domain_id, d.system, c.identifier_sealed, c.identifier_value,
                        1, NULL::uuid, c.identifier_hmac
                 FROM pix.link l
                 JOIN pix.source_record r ON r.id = l.record_id
                 JOIN pix.carried_identifier c
                   ON c.record_id = r.id AND c.version = r.head_version
                 JOIN pix.identifier_domain d ON d.id = c.domain_id
                 WHERE l.person_id = (SELECT id FROM pick) AND upper_inf(l.valid)
                   AND r.active AND r.removed_at IS NULL
                   AND d.kind = 'authoritative' AND d.retired_at IS NULL
                   AND NOT EXISTS (
                       SELECT 1 FROM pix.source_record o
                       WHERE o.domain_id = c.domain_id
                         AND o.identifier_hmac = c.identifier_hmac
                   )
                 ORDER BY d.system, c.identifier_hmac
             ) carried
         )"
    };
}

/// The statement of [`query`]: `$1` the domain, `$2` the HMAC (`NULL` for a
/// value the domain's rule refuses), `$3` the hops, `$4` the target systems
/// and `$5` whether the pseudonymisation domains are read. Each row's first
/// column names its part: 0 the domain, 1 a person found, 2 an identifier
/// of the one person found, 3 a target, 4 a pseudonymisation domain.
const QUERY: &str = concat!(
    persons_ctes!(),
    ",
     pick (id) AS (
         SELECT id FROM found WHERE (SELECT count(*) FROM found) = 1
     ),
     ",
    identified_cte!(),
    "
     SELECT part, id, text, sealed, plain, client, flag, value FROM (
         SELECT 0 AS part, d.id AS id, NULL::text AS text, NULL::bytea AS sealed,
                NULL::text AS plain, NULL::text AS client,
                d.retired_at IS NOT NULL AS flag, NULL::uuid AS value, 0::bigint AS ord
         FROM pix.identifier_domain d WHERE d.id = $1
         UNION ALL
         SELECT 1, id, NULL, NULL, NULL, NULL, NULL, NULL, row_number() OVER (ORDER BY id)
         FROM found
         UNION ALL
         SELECT 2, domain_id, system, sealed, plain, NULL, NULL, NULL,
                row_number() OVER (ORDER BY part, rid, hmac)
         FROM identified
         UNION ALL
         SELECT 3, NULL, t.system, NULL, NULL, NULL,
                EXISTS (
                    SELECT 1 FROM pix.identifier_domain d
                    WHERE d.system = t.system AND d.retired_at IS NULL
                ),
                NULL, t.ord
         FROM unnest($4::text[]) WITH ORDINALITY AS t (system, ord)
         UNION ALL
         SELECT 4, p.id, p.namespace, NULL, NULL, p.consumer_client_id,
                p.retired_at IS NOT NULL, s.value,
                row_number() OVER (ORDER BY p.namespace)
         FROM pix.pseudonym_domain p
         LEFT JOIN pix.pseudonym s
           ON s.domain_id = p.id AND s.person_id = (SELECT id FROM pick)
         WHERE $5 AND (
             p.namespace = ANY($4::text[])
             OR (cardinality($4::text[]) = 0
                 AND s.value IS NOT NULL AND p.retired_at IS NULL)
         )
     ) parts
     ORDER BY part, ord",
);

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

/// What a PIX query asks of the store.
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    /// The system of the identifier's domain.
    pub system: &'a str,
    /// The identifier's value, as the request carries it.
    pub value: &'a str,
    /// The systems the query names as target domains.
    pub targets: &'a [&'a str],
    /// Whether the pseudonymisation domains are read: the ones `targets`
    /// names, and with no target the unretired ones the person holds a
    /// pseudonym in.
    pub pseudonyms: bool,
}

/// What the store answered a PIX query, from one snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    /// The cross-reference of the identifier.
    pub lookup: Lookup,
    /// Each target system asked, in the order asked, with whether an
    /// unretired identifier domain has it as its system; empty for
    /// [`Lookup::UnknownDomain`].
    pub targets: Vec<(String, bool)>,
    /// The pseudonymisation domains [`Ask::pseudonyms`] reads, by
    /// namespace, each with the pseudonym the person found holds in it.
    pub pseudonymised: Vec<(PseudonymDomain, Option<Uuid>)>,
}

impl Answered {
    /// The answer for a domain the store does not hold.
    const UNKNOWN: Self = Self {
        lookup: Lookup::UnknownDomain,
        targets: Vec::new(),
        pseudonymised: Vec::new(),
    };
}

/// The id of each identifier domain read, by system.
#[derive(Debug, Default)]
pub struct DomainIds(RwLock<BTreeMap<String, DomainId>>);

impl DomainIds {
    /// Returns the id kept for `system`.
    fn get(&self, system: &str) -> Option<DomainId> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(system)
            .copied()
    }

    /// Keeps `id` for `system`.
    fn keep(&self, system: &str, id: DomainId) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(system.to_owned(), id);
    }

    /// Reads the id of `system` from the store and keeps it; `None` when no
    /// domain has the system.
    async fn fetch(
        &self,
        connection: &mut PgConnection,
        system: &str,
    ) -> Result<Option<DomainId>, StoreError> {
        let id = record::domain(&mut *connection, system)
            .await?
            .map(|domain| domain.id);
        match id {
            Some(id) => self.keep(system, id),
            None => {
                self.0
                    .write()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(system);
            }
        }
        Ok(id)
    }
}

/// An identifier of the person as stored: domain, system, sealed value,
/// plain value.
type IdentifierRow = (Uuid, String, Option<Vec<u8>>, Option<String>);

/// One row of [`QUERY`]: part, id, text, sealed, plain, client, flag, value.
type QueryRow = (
    i32,
    Option<Uuid>,
    Option<String>,
    Option<Vec<u8>>,
    Option<String>,
    Option<String>,
    Option<bool>,
    Option<Uuid>,
);

/// Reads the cross-reference `ask` names, with its targets and the
/// pseudonymisation domains it asks for, in one statement.
///
/// The first query of a domain sends one statement more, which reads its
/// id; so does each query of a system no domain has, which answers
/// [`Lookup::UnknownDomain`] after it.
///
/// # Errors
/// Returns [`StoreError::Query`] when a statement fails or answers a row of
/// a shape it cannot, [`StoreError::UnknownDomainKind`] for a domain of a
/// kind the schema does not admit, and [`StoreError::Seal`] when a sealed
/// value does not open under the store's keys.
pub async fn query(
    connection: &mut PgConnection,
    store: &Store,
    ask: &Ask<'_>,
) -> Result<Answered, StoreError> {
    let ids = store.domain_ids();
    let kept = ids.get(ask.system);
    let Some(domain) = (match kept {
        Some(id) => Some(id),
        None => ids.fetch(&mut *connection, ask.system).await?,
    }) else {
        return Ok(Answered::UNKNOWN);
    };
    let rows = read(&mut *connection, store.sealer(), domain, ask).await?;
    if let Some(answered) = assemble(rows, store.sealer(), ask)? {
        return Ok(answered);
    }
    // NOTE: no specification governs this: our own design; an id kept for a
    // system the store no longer holds under it is read again, once.
    if kept.is_none() {
        return Ok(Answered::UNKNOWN);
    }
    let Some(domain) = ids.fetch(&mut *connection, ask.system).await? else {
        return Ok(Answered::UNKNOWN);
    };
    let rows = read(connection, store.sealer(), domain, ask).await?;
    Ok(assemble(rows, store.sealer(), ask)?.unwrap_or(Answered::UNKNOWN))
}

/// Sends [`QUERY`] for `ask` in the domain `domain`.
async fn read(
    connection: &mut PgConnection,
    keys: &Sealer,
    domain: DomainId,
    ask: &Ask<'_>,
) -> Result<Vec<QueryRow>, StoreError> {
    // NOTE: PIXm 3.1.0 §2:3.83.4.2.2.2; a value its domain's rule refuses
    // names no record the store can hold, so it binds no HMAC and is not found.
    let hmac = Rule::of(ask.system)
        .normalise(ask.value)
        .ok()
        .map(|value| keys.hmac(domain.uuid(), &value).to_vec());
    let rows: Vec<QueryRow> = sqlx::query_as(QUERY)
        .bind(domain.uuid())
        .bind(hmac)
        .bind(MAX_HOPS)
        .bind(ask.targets)
        .bind(ask.pseudonyms)
        .fetch_all(&mut *connection)
        .await?;
    Ok(rows)
}

/// Assembles the rows of [`QUERY`]; `None` when the domain row is missing.
fn assemble(
    rows: Vec<QueryRow>,
    keys: &Sealer,
    ask: &Ask<'_>,
) -> Result<Option<Answered>, StoreError> {
    let mut retired = None;
    let mut persons = Vec::new();
    let mut held = Vec::new();
    let mut targets = Vec::with_capacity(ask.targets.len());
    let mut pseudonymised = Vec::new();
    for (part, id, text, sealed, plain, client, flag, value) in rows {
        match (part, id, text, flag) {
            (0, Some(_), None, Some(flag)) => retired = Some(flag),
            (1, Some(person), None, None) => persons.push(person),
            (2, Some(domain), Some(system), None) => held.push((domain, system, sealed, plain)),
            (3, None, Some(system), Some(flag)) => targets.push((system, flag)),
            (4, Some(id), Some(namespace), Some(flag)) => pseudonymised.push((
                PseudonymDomain {
                    id,
                    namespace,
                    consumer_client_id: client,
                    retired: flag,
                },
                value,
            )),
            _ => {
                return Err(StoreError::from(sqlx::Error::Protocol(String::from(
                    "a cross-reference row of an unknown shape",
                ))));
            }
        }
    }
    let Some(retired) = retired else {
        return Ok(None);
    };
    let lookup = if retired {
        Lookup::UnknownDomain
    } else {
        match persons.as_slice() {
            [] => Lookup::NotFound,
            [person] => Lookup::Found(CrossReference {
                person: PersonId::from_uuid(*person),
                identifiers: open(held, keys)?,
            }),
            _ => Lookup::Ambiguous,
        }
    };
    Ok(Some(Answered {
        lookup,
        targets,
        pseudonymised,
    }))
}

/// Reads every active person an identifier is cross-referenced to.
///
/// The identifier is the one of HMAC `hmac` in the domain `domain`, and the
/// persons come in id order: the person of its record,
/// followed to the survivor of a subsumed record and a merged person, or the
/// persons of the current versions that carry it when it is authoritative
/// and has no record of its own.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn persons(
    connection: &mut PgConnection,
    domain: Uuid,
    hmac: &[u8],
) -> Result<Vec<Uuid>, StoreError> {
    let persons: Vec<Uuid> = sqlx::query_scalar(concat!(
        persons_ctes!(),
        " SELECT id FROM found ORDER BY id"
    ))
    .bind(domain)
    .bind(hmac)
    .bind(MAX_HOPS)
    .fetch_all(&mut *connection)
    .await?;
    Ok(persons)
}

/// Reads every identifier of `person`: its records' own and the
/// authoritative ones they carry with no record of their own, ordered by
/// system, in one statement.
///
/// The PDQm Supplier serves the same identifiers in each `Patient`, so a
/// person answers alike in ITI-83 and ITI-78.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails, and
/// [`StoreError::Seal`] when a sealed value does not open under `keys`.
pub async fn identifiers(
    connection: &mut PgConnection,
    keys: &Sealer,
    person: Uuid,
) -> Result<Vec<(String, IdentifierValue)>, StoreError> {
    let rows: Vec<IdentifierRow> = sqlx::query_as(concat!(
        "WITH pick (id) AS (SELECT $1::uuid), ",
        identified_cte!(),
        " SELECT domain_id, system, sealed, plain FROM identified ORDER BY part, rid, hmac"
    ))
    .bind(person)
    .fetch_all(&mut *connection)
    .await?;
    open(rows, keys)
}

/// Opens each stored identifier of `rows`, ordered by system: the stable
/// sort keeps a record's own before the carried ones of the same system.
fn open(
    mut rows: Vec<IdentifierRow>,
    keys: &Sealer,
) -> Result<Vec<(String, IdentifierValue)>, StoreError> {
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
