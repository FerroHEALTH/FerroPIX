// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Source records, found by their identifier in one domain.
//!
//! A source record is what one domain said about one patient: its
//! identifier in that domain and, version by version, the demographics the
//! feed carried, the elements of the PDQm Patient profile the Manager
//! accepts (PIXm 3.1.0 §2:3.104.4.1.2.1). The identifier is written as its
//! HMAC, and sealed in a sensitive domain, so the lookup PIXm needs (the
//! identifiers that match a source identifier, PIXm 3.1.0 §2:3.83.4.1.3) is
//! one probe of the `(domain_id, identifier_hmac)` index. No specification
//! governs the storage: our own design (`docs/architecture.md` §4.2).

use std::fmt;

use serde_json::value::RawValue;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::config::section::domain::DomainKind;
use crate::store::StoreError;
use crate::store::seal::{IdentifierValue, Sealer};

/// The id of an identifier domain in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DomainId(Uuid);

impl DomainId {
    /// Returns the uuid.
    #[must_use]
    pub fn uuid(self) -> Uuid {
        self.0
    }
}

/// The id of a source record in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordId(Uuid);

impl RecordId {
    /// Returns the uuid.
    #[must_use]
    pub fn uuid(self) -> Uuid {
        self.0
    }

    /// Returns the id of the record the store calls `uuid`.
    pub(crate) const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

/// The id of a person, the cross-reference: the FHIR `Patient.id` the face
/// serves (`docs/architecture.md` §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PersonId(Uuid);

impl PersonId {
    /// Returns the uuid.
    #[must_use]
    pub fn uuid(self) -> Uuid {
        self.0
    }

    /// Returns the id of the person the store calls `uuid`.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

/// An identifier domain as the record functions need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Domain {
    /// The domain's id.
    pub id: DomainId,
    /// The domain's system.
    pub system: String,
    /// What the domain's identifiers are.
    pub kind: DomainKind,
    /// Whether its identifier values are sealed at rest.
    pub sensitive: bool,
    /// Whether the domain is retired, so it answers as unknown.
    pub retired: bool,
}

/// The channel a version arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// The FHIR face: ITI-104.
    Fhir,
    /// The HL7 v2 face: ITI-8.
    V2,
    /// The PMIR feed: ITI-93.
    Pmir,
}

impl Channel {
    /// Returns the stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fhir => "fhir",
            Self::V2 => "v2",
            Self::Pmir => "pmir",
        }
    }
}

/// The demographics one version of a record carries: the JSON text of an
/// object, stored as `jsonb`.
///
/// Its `Debug` shows only the length of the text, so no name, birth date or
/// address reaches a log line.
#[derive(Clone)]
pub struct Demographics(Box<RawValue>);

impl Demographics {
    /// Reads the JSON text `json`, which the store holds to be an object.
    ///
    /// # Errors
    /// Returns the parser's error, which names a line and a column and
    /// quotes nothing, when `json` is not JSON.
    pub fn from_json(json: String) -> Result<Self, serde_json::Error> {
        RawValue::from_string(json).map(Self)
    }

    /// Returns the JSON text, for the one place it is used.
    ///
    /// Never log or format the result.
    #[must_use]
    pub fn expose(&self) -> &str {
        self.0.get()
    }
}

impl fmt::Debug for Demographics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Demographics(*** {} bytes)", self.0.get().len())
    }
}

/// Reads the domain whose system is `system`.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails, and
/// [`StoreError::UnknownDomainKind`] for a kind the schema does not admit.
pub async fn domain<'c, E>(executor: E, system: &str) -> Result<Option<Domain>, StoreError>
where
    E: sqlx::PgExecutor<'c>,
{
    let row: Option<DomainRow> = sqlx::query_as(
        "SELECT id, system, kind, sensitive, retired_at IS NOT NULL
         FROM pix.identifier_domain WHERE system = $1",
    )
    .bind(system)
    .fetch_optional(executor)
    .await?;
    row.map(read_domain).transpose()
}

/// Reads every domain the store holds, retired ones included, by system.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails, and
/// [`StoreError::UnknownDomainKind`] for a kind the schema does not admit.
pub async fn domains<'c, E>(executor: E) -> Result<Vec<Domain>, StoreError>
where
    E: sqlx::PgExecutor<'c>,
{
    let rows: Vec<DomainRow> = sqlx::query_as(
        "SELECT id, system, kind, sensitive, retired_at IS NOT NULL
         FROM pix.identifier_domain ORDER BY system",
    )
    .fetch_all(executor)
    .await?;
    rows.into_iter().map(read_domain).collect()
}

/// A domain row: id, system, kind, sensitive, retired.
type DomainRow = (Uuid, String, String, bool, bool);

/// Reads one domain row.
fn read_domain((id, system, kind, sensitive, retired): DomainRow) -> Result<Domain, StoreError> {
    let kind = match kind.as_str() {
        "local" => DomainKind::Local,
        "authoritative" => DomainKind::Authoritative,
        "master" => DomainKind::Master,
        _ => return Err(StoreError::UnknownDomainKind { system }),
    };
    Ok(Domain {
        id: DomainId(id),
        system,
        kind,
        sensitive,
        retired,
    })
}

/// Inserts a record for `value` in `domain` with its first version, inside
/// the caller's transaction.
///
/// The identifier is written as its HMAC beside its sealed form in a
/// sensitive domain and its plain form in another, under the key `keys`
/// holds.
///
/// # Errors
/// Returns [`StoreError::Seal`] when the value cannot be sealed, and
/// [`StoreError::Query`] when a statement fails, a record of the same
/// identifier in the domain included (the constraint
/// `source_record_identifier_key`).
pub async fn create(
    connection: &mut PgConnection,
    keys: &Sealer,
    domain: &Domain,
    value: &IdentifierValue,
    demographics: &Demographics,
    channel: Channel,
    request_id: Option<&str>,
) -> Result<RecordId, StoreError> {
    let domain_id = domain.id.uuid();
    let hmac = keys.hmac(domain_id, value);
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
    sqlx::query(
        "INSERT INTO pix.source_record_version (record_id, version, demographics, channel, request_id)
         VALUES ($1, 1, $2, $3, $4)",
    )
    .bind(id)
    .bind(sqlx::types::Json(&demographics.0))
    .bind(channel.as_str())
    .bind(request_id)
    .execute(&mut *connection)
    .await?;
    Ok(RecordId(id))
}

/// Finds the record of `value` in `domain`, removed or not.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn find<'c, E>(
    executor: E,
    keys: &Sealer,
    domain: &Domain,
    value: &IdentifierValue,
) -> Result<Option<RecordId>, StoreError>
where
    E: sqlx::PgExecutor<'c>,
{
    let hmac = keys.hmac(domain.id.uuid(), value);
    let id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM pix.source_record WHERE domain_id = $1 AND identifier_hmac = $2",
    )
    .bind(domain.id.uuid())
    .bind(hmac.as_slice())
    .fetch_optional(executor)
    .await?;
    Ok(id.map(RecordId))
}

/// Reads the identifier value of `record`, opening it when it is sealed.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails or no such record
/// exists, and [`StoreError::Seal`] when the sealed value does not open under
/// the key `keys` holds.
pub async fn identifier<'c, E>(
    executor: E,
    keys: &Sealer,
    record: RecordId,
) -> Result<IdentifierValue, StoreError>
where
    E: sqlx::PgExecutor<'c>,
{
    let (domain_id, sealed, plain): (Uuid, Option<Vec<u8>>, Option<String>) = sqlx::query_as(
        "SELECT domain_id, identifier_sealed, identifier_value
         FROM pix.source_record WHERE id = $1",
    )
    .bind(record.uuid())
    .fetch_one(executor)
    .await?;
    match (sealed, plain) {
        (Some(sealed), _) => Ok(keys.open(domain_id, &sealed)?),
        (None, Some(plain)) => IdentifierValue::new(plain)
            .ok_or(StoreError::Seal(crate::store::seal::SealError::Malformed)),
        (None, None) => Err(StoreError::Seal(crate::store::seal::SealError::Malformed)),
    }
}

#[cfg(test)]
mod tests {
    use super::Demographics;

    #[test]
    fn the_debug_of_demographics_shows_no_value() {
        let family = ferropix_testkit::synthetic::FAMILY_NAMES[0];
        let given = ferropix_testkit::synthetic::GIVEN_NAMES[0];
        let json = format!(r#"{{"name":[{{"family":"{family}","given":["{given}"]}}]}}"#);
        let length = json.len();
        let rendered = format!("{:?}", Demographics::from_json(json).unwrap());
        assert_eq!(format!("Demographics(*** {length} bytes)"), rendered);
        assert!(Demographics::from_json(String::from(r#"{"name":"#)).is_err());
    }
}
