// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pseudonyms: the UUID a person carries in a pseudonymisation domain,
//! minted on first ask, immutable and attached to the person it was minted
//! for.
//!
//! A person is resolved to its pseudonym through `merged_into`: the caller
//! passes the active survivor, so after a merge both persons' identifiers
//! answer the survivor's pseudonym, and after an unmerge each its own. The
//! reverse resolution follows the person a pseudonym was minted for to its
//! survivor the same way. A mint is an insert that returns the row already
//! there, so two first asks at once yield one pseudonym: the primary key
//! `(person_id, domain_id)` holds it. No specification governs the
//! pseudonym: our own design.

use sqlx::PgConnection;
use uuid::Uuid;

use crate::config::section::pseudonym::PseudonymSeed;
use crate::store::record::PersonId;
use crate::store::{Store, StoreError};

/// The hops a chain of merged persons is followed.
const MAX_HOPS: i32 = 64;

/// One pseudonymisation domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PseudonymDomain {
    /// The domain's id.
    pub id: Uuid,
    /// The namespace, the pseudonyms' system.
    pub namespace: String,
    /// The client id that reads and mints the domain's pseudonyms through
    /// ITI-83, when one is.
    pub consumer_client_id: Option<String>,
    /// Whether the domain is retired: it mints nothing and answers as
    /// unknown.
    pub retired: bool,
}

impl PseudonymDomain {
    /// Returns whether a caller of client id `client` reads and mints the
    /// domain's pseudonyms through ITI-83: only the domain's consumer does,
    /// so no other Consumer can link two domains.
    #[must_use]
    pub fn serves(&self, client: Option<&str>) -> bool {
        !self.retired && client.is_some() && self.consumer_client_id.as_deref() == client
    }
}

/// A person's pseudonym in one domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    /// The pseudonym.
    pub value: Uuid,
    /// Whether this call minted it.
    pub minted: bool,
}

/// Reads every pseudonymisation domain, by namespace.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn domains(connection: &mut PgConnection) -> Result<Vec<PseudonymDomain>, StoreError> {
    let rows: Vec<(Uuid, String, Option<String>, bool)> = sqlx::query_as(
        "SELECT id, namespace, consumer_client_id, retired_at IS NOT NULL
         FROM pix.pseudonym_domain ORDER BY namespace",
    )
    .fetch_all(&mut *connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, namespace, consumer_client_id, retired)| PseudonymDomain {
                id,
                namespace,
                consumer_client_id,
                retired,
            },
        )
        .collect())
}

/// Returns the pseudonym of the active person `person` in the domain
/// `domain`, minting one when it has none and `mint` holds; `None` when it
/// has none and `mint` does not hold.
///
/// # Errors
/// Returns [`StoreError::Query`] when a statement fails.
pub async fn resolve(
    connection: &mut PgConnection,
    person: PersonId,
    domain: Uuid,
    mint: bool,
) -> Result<Option<Resolved>, StoreError> {
    if mint {
        let minted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO pix.pseudonym (person_id, domain_id, value) VALUES ($1, $2, $3)
             ON CONFLICT (person_id, domain_id) DO NOTHING
             RETURNING value",
        )
        .bind(person.uuid())
        .bind(domain)
        .bind(Uuid::new_v4())
        .fetch_optional(&mut *connection)
        .await?;
        if let Some(value) = minted {
            return Ok(Some(Resolved {
                value,
                minted: true,
            }));
        }
    }
    let held: Option<Uuid> = sqlx::query_scalar(
        "SELECT value FROM pix.pseudonym WHERE person_id = $1 AND domain_id = $2",
    )
    .bind(person.uuid())
    .bind(domain)
    .fetch_optional(&mut *connection)
    .await?;
    Ok(held.map(|value| Resolved {
        value,
        minted: false,
    }))
}

/// Returns every pseudonym the active person `person` holds in an unretired
/// domain, as `(namespace, value)`, by namespace.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn held(
    connection: &mut PgConnection,
    person: PersonId,
) -> Result<Vec<(String, Uuid)>, StoreError> {
    let rows: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT d.namespace, p.value FROM pix.pseudonym p
         JOIN pix.pseudonym_domain d ON d.id = p.domain_id
         WHERE p.person_id = $1 AND d.retired_at IS NULL
         ORDER BY d.namespace",
    )
    .bind(person.uuid())
    .fetch_all(&mut *connection)
    .await?;
    Ok(rows)
}

/// Returns the active person the pseudonym `value` of the domain
/// `namespace` resolves to: the person it was minted for, followed through
/// `merged_into` to its survivor; `None` when no unretired domain holds it.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn reverse(
    connection: &mut PgConnection,
    namespace: &str,
    value: Uuid,
) -> Result<Option<PersonId>, StoreError> {
    let person: Option<Uuid> = sqlx::query_scalar(
        "WITH RECURSIVE chain (id, merged_into, hops) AS (
             SELECT p.id, p.merged_into, 0 FROM pix.person p
             JOIN pix.pseudonym s ON s.person_id = p.id
             JOIN pix.pseudonym_domain d ON d.id = s.domain_id
             WHERE d.namespace = $1 AND s.value = $2 AND d.retired_at IS NULL
             UNION ALL
             SELECT p.id, p.merged_into, c.hops + 1
             FROM pix.person p JOIN chain c ON p.id = c.merged_into
             WHERE c.hops < $3
         )
         SELECT id FROM chain WHERE merged_into IS NULL",
    )
    .bind(namespace)
    .bind(value)
    .bind(MAX_HOPS)
    .fetch_optional(&mut *connection)
    .await?;
    Ok(person.map(PersonId::from_uuid))
}

/// Inserts every seeded pseudonymisation domain the store lacks, in one
/// transaction, and leaves a present one as the store holds it.
///
/// # Errors
/// Returns [`StoreError::Query`] when a statement fails.
pub async fn seed(store: &Store, seeds: &[PseudonymSeed]) -> Result<usize, StoreError> {
    let mut transaction = store.pool().begin().await?;
    let mut inserted = 0;
    for seed in seeds {
        let added = sqlx::query(
            "INSERT INTO pix.pseudonym_domain (namespace, name, consumer_client_id)
             VALUES ($1, $2, $3)
             ON CONFLICT (namespace) DO NOTHING",
        )
        .bind(&seed.namespace)
        .bind(&seed.name)
        .bind(seed.consumer_client_id.as_deref())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if added == 1 {
            inserted += 1;
            tracing::info!(
                namespace = seed.namespace.as_str(),
                "the pseudonymisation domain is seeded"
            );
        }
    }
    transaction.commit().await?;
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::PseudonymDomain;

    #[test]
    fn only_the_named_consumer_is_served_and_a_retired_domain_serves_no_one() {
        let mut domain = PseudonymDomain {
            id: Uuid::nil(),
            namespace: String::from("urn:oid:2.999.9.1"),
            consumer_client_id: Some(String::from("ferroehr")),
            retired: false,
        };
        assert!(domain.serves(Some("ferroehr")));
        assert!(!domain.serves(Some("other")));
        assert!(!domain.serves(None));
        domain.retired = true;
        assert!(!domain.serves(Some("ferroehr")));
        domain.retired = false;
        domain.consumer_client_id = None;
        assert!(!domain.serves(Some("ferroehr")));
    }
}
