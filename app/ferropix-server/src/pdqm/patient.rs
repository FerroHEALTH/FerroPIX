// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `Patient` the Supplier serves: the person, with every identifier of
//! every open link and the demographics of one of its records.
//!
//! `Patient.id` is the person's id. Its identifiers are the person's id in
//! the Master Patient Identification Domain, `public_base_url`
//! (PMIR 1.6.0 §2:3.93.4.1.3), and the identifiers a PIX query answers for
//! the person ([`crate::store::xref::identifiers`]), each in its domain's
//! system with its domain's identifier type, opened from its sealed form for
//! the answer alone. Its demographics are the current version of
//! one record: an `authoritative` domain's first, then the most recently
//! received. A merged person reads `active = false` with a `replaced-by`
//! link to its active survivor (FHIR R4 Patient, "Patient records that have
//! been merged"). No specification fixes what a Supplier's `Patient` is:
//! our own design (`docs/architecture.md` §6, decision D2).

use std::collections::BTreeMap;

use ferropix_operator_api::patient::IdentifierType;
use fhir_types::r4::codeable_concept::CodeableConcept;
use fhir_types::r4::coding::Coding;
use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::meta::Meta;
use fhir_types::r4::patient::{Patient, PatientLink};
use fhir_types::r4::reference::Reference;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::pdqm::PdqError;
use crate::store::demographics;
use crate::store::seal::Sealer;
use crate::store::xref;

/// The canonical URL of the PDQm Patient profile every served `Patient`
/// claims (PDQm 3.2.0 §2:3.78.4.2.2.1).
pub const PDQM_PATIENT: &str =
    "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Patient";

/// The hops a chain of merged persons is followed.
const MAX_HOPS: i32 = 64;

/// The demographics each person serves, one row per person: the person,
/// the head version of the record `pix.person_served` names, which
/// `pix.serve` keeps at the end of every statement that changes it (#722).
///
/// The ITI-78 search reads the same rows, so a search matches the values
/// the answer shows.
pub(crate) const SERVED: &str = "SELECT s.person_id, v.demographics::text
     FROM pix.person_served s
     JOIN pix.source_record r ON r.id = s.record_id
     JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version";

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
        "{SERVED} WHERE s.person_id = ANY($1)"
    )))
    .bind(&ids)
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    let mut served: BTreeMap<Uuid, String> = served.into_iter().collect();
    let mut identifiers: BTreeMap<Uuid, Vec<Identifier>> = BTreeMap::new();
    for person in persons {
        let held = xref::identifiers(&mut *connection, keys, person.id).await?;
        let id = person.id.to_string();
        // NOTE: PMIR 1.6.0 §2:3.93.4.1.3: the master domain is the Registry's base URL, so its record repeats the master identifier.
        identifiers.insert(
            person.id,
            held.iter()
                .filter(|held| !(held.system == master && held.value.expose() == id))
                .map(|held| identifier(&held.system, held.kind, held.value.expose()))
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
        patient.identifier = std::iter::once(identifier(master, IdentifierType::Pi, &id))
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

/// The HL7 Identifier Type code system.
pub const IDENTIFIER_TYPE: &str = "http://terminology.hl7.org/CodeSystem/v2-0203";

/// Returns the identifier `value` in `system`, of the type `kind`.
pub(crate) fn identifier(system: &str, kind: IdentifierType, value: &str) -> Identifier {
    Identifier {
        r#type: Some(identifier_type(kind)),
        system: Some(system.into()),
        value: Some(value.into()),
        ..Identifier::default()
    }
}

/// Returns `Identifier.type` for `kind`: a coding of the HL7 Identifier Type,
/// or the code as text for an EHDS code no published code system holds.
// NOTE: draft EEHRxF Annexes, Table 26: EHDSPatient.identifier.type extends the HL7
// Identifier Type with HI, HIN, REGPID and REGION, and names no code system for them.
pub(crate) fn identifier_type(kind: IdentifierType) -> CodeableConcept {
    match kind.v2_display() {
        Some(display) => CodeableConcept {
            coding: vec![Coding {
                system: Some(IDENTIFIER_TYPE.into()),
                code: Some(kind.code().into()),
                display: Some(display.into()),
                ..Coding::default()
            }],
            ..CodeableConcept::default()
        },
        None => CodeableConcept {
            text: Some(kind.code().into()),
            ..CodeableConcept::default()
        },
    }
}

/// Wraps a driver error.
fn store(source: sqlx::Error) -> PdqError {
    PdqError::Store(source.into())
}

#[cfg(test)]
mod tests {
    use ferropix_operator_api::patient::IdentifierType;

    use super::{IDENTIFIER_TYPE, identifier_type};

    #[test]
    fn a_v2_code_is_a_coding_and_an_ehds_code_is_text() {
        for kind in IdentifierType::ALL {
            let concept = identifier_type(kind);
            match kind {
                IdentifierType::Hi
                | IdentifierType::Hin
                | IdentifierType::Regpid
                | IdentifierType::Region => {
                    assert!(concept.coding.is_empty(), "{kind}: no code system holds it");
                    assert_eq!(
                        Some(kind.code()),
                        concept
                            .text
                            .as_ref()
                            .and_then(|value| value.value.as_deref()),
                        "{kind}"
                    );
                }
                _ => {
                    let [coding] = concept.coding.as_slice() else {
                        panic!("{kind}: one coding");
                    };
                    assert_eq!(
                        Some(IDENTIFIER_TYPE),
                        coding
                            .system
                            .as_ref()
                            .and_then(|value| value.value.as_deref()),
                        "{kind}"
                    );
                    assert_eq!(
                        Some(kind.code()),
                        coding
                            .code
                            .as_ref()
                            .and_then(|value| value.value.as_deref()),
                        "{kind}"
                    );
                    assert!(coding.display.is_some(), "{kind}");
                    assert_eq!(
                        None,
                        concept
                            .text
                            .as_ref()
                            .and_then(|value| value.value.as_deref()),
                        "{kind}"
                    );
                }
            }
        }
    }
}
