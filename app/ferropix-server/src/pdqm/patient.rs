// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `Patient` the Supplier serves: the person, with every identifier of
//! every open link and the demographics of one of its records.
//!
//! `Patient.id` is the person's id. Its identifiers are the person's id in
//! the Master Patient Identification Domain, `[server] public_base_url`
//! (PMIR 1.6.0 §2:3.93.4.1.3), and the identifiers a PIX query answers for
//! the person ([`crate::store::xref::identifiers`]), each in its domain's
//! system, opened from its sealed form for the answer alone. Its demographics are the current version of
//! one record: an `authoritative` domain's first, then the most recently
//! received. A merged person reads `active = false` with a `replaced-by`
//! link to its active survivor (FHIR R4 Patient, "Patient records that have
//! been merged"). No specification fixes what a Supplier's `Patient` is:
//! our own design (`docs/architecture.md` §6, decision D2).

use std::collections::BTreeMap;

use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::meta::Meta;
use fhir_types::r4::patient::{Patient, PatientLink};
use fhir_types::r4::reference::Reference;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::feed::demographics;
use crate::pdqm::PdqError;
use crate::store::seal::Sealer;
use crate::store::xref;

/// The canonical URL of the PDQm Patient profile every served `Patient`
/// claims (PDQm 3.2.0 §2:3.78.4.2.2.1).
pub const PDQM_PATIENT: &str =
    "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Patient";

/// The hops a chain of merged persons is followed.
const MAX_HOPS: i32 = 64;

/// The open links, active records and current versions the served
/// demographics are chosen from, one row per person: the person, the
/// demographics.
///
/// The ITI-78 search reads the same rows, so a search matches the values
/// the answer shows.
pub(crate) const SERVED: &str = "SELECT DISTINCT ON (l.person_id) l.person_id, v.demographics
     FROM pix.link l
     JOIN pix.source_record r ON r.id = l.record_id
     JOIN pix.identifier_domain d ON d.id = r.domain_id
     JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
     WHERE upper_inf(l.valid) AND r.active AND r.removed_at IS NULL AND d.retired_at IS NULL";

/// The order [`SERVED`] picks a person's record in.
pub(crate) const SERVED_ORDER: &str =
    "ORDER BY l.person_id, (d.kind = 'authoritative') DESC, v.received_at DESC, r.id";

/// One person as the store holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Person {
    /// The person's id.
    pub id: Uuid,
    /// The active survivor of a merged person, `None` for an active one.
    pub survivor: Option<Uuid>,
}

impl Person {
    /// Returns whether the person was merged into another.
    #[must_use]
    pub const fn merged(&self) -> bool {
        self.survivor.is_some()
    }
}

/// Reads the persons of `ids`, each with its active survivor when it was
/// merged; an id the store holds no person of is left out.
///
/// # Errors
/// Returns [`PdqError::Store`] when the statement fails.
pub async fn persons(
    connection: &mut PgConnection,
    ids: &[Uuid],
) -> Result<BTreeMap<Uuid, Person>, PdqError> {
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "WITH RECURSIVE chain (start, id, merged_into, hops) AS (
             SELECT id, id, merged_into, 0 FROM pix.person WHERE id = ANY($1)
             UNION ALL
             SELECT c.start, p.id, p.merged_into, c.hops + 1
             FROM pix.person p JOIN chain c ON p.id = c.merged_into
             WHERE c.hops < $2
         )
         SELECT start, id FROM chain WHERE merged_into IS NULL",
    )
    .bind(ids)
    .bind(MAX_HOPS)
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    Ok(rows
        .into_iter()
        .map(|(id, survivor)| {
            let person = Person {
                id,
                survivor: (survivor != id).then_some(survivor),
            };
            (id, person)
        })
        .collect())
}

/// Returns the served `Patient` of each person of `persons`, by id, with
/// `master` the system of the Master Patient Identification Domain and
/// `keys` the keys that open a sealed identifier.
///
/// # Errors
/// Returns [`PdqError::Store`] when a statement fails or a sealed value does
/// not open under `keys`, and [`PdqError::Demographics`] when stored
/// demographics are no `Patient`.
pub async fn patients(
    connection: &mut PgConnection,
    keys: &Sealer,
    master: &str,
    persons: &[Person],
) -> Result<BTreeMap<Uuid, Patient>, PdqError> {
    let ids: Vec<Uuid> = persons.iter().map(|person| person.id).collect();
    let served: Vec<(Uuid, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT person_id, demographics::text FROM ({SERVED} AND l.person_id = ANY($1) {SERVED_ORDER}) s"
    )))
    .bind(&ids)
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    let mut served: BTreeMap<Uuid, String> = served.into_iter().collect();
    let mut identifiers: BTreeMap<Uuid, Vec<Identifier>> = BTreeMap::new();
    for person in persons {
        let held = xref::identifiers(&mut *connection, keys, person.id).await?;
        identifiers.insert(
            person.id,
            held.iter()
                .map(|(system, value)| identifier(system, value.expose()))
                .collect(),
        );
    }
    let mut patients = BTreeMap::new();
    for person in persons {
        let mut patient = match served.remove(&person.id) {
            Some(text) => demographics::patient(&text)?,
            None => Patient::default(),
        };
        let id = person.id.to_string();
        patient.id = Some(id.clone());
        patient.meta = Some(Meta {
            profile: vec![PDQM_PATIENT.into()],
            ..Meta::default()
        });
        patient.identifier = std::iter::once(identifier(master, &id))
            .chain(identifiers.remove(&person.id).unwrap_or_default())
            .collect();
        patient.active = Some((!person.merged()).into());
        patient.link = person
            .survivor
            .map(|survivor| PatientLink {
                other: Reference {
                    reference: Some(format!("Patient/{survivor}").as_str().into()),
                    ..Reference::default()
                },
                r#type: "replaced-by".into(),
                ..PatientLink::default()
            })
            .into_iter()
            .collect();
        patients.insert(person.id, patient);
    }
    Ok(patients)
}

/// Returns the identifier `value` in `system`.
fn identifier(system: &str, value: &str) -> Identifier {
    Identifier {
        system: Some(system.into()),
        value: Some(value.into()),
        ..Identifier::default()
    }
}

/// Wraps a driver error.
fn store(source: sqlx::Error) -> PdqError {
    PdqError::Store(source.into())
}
