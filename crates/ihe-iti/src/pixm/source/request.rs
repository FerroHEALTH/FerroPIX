// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-104 requests: the conditional update `PUT
//! [base]/Patient?identifier=system|value` with a Patient body
//! (§2:3.104.4.1.2, §2:3.104.4.2.2) and the conditional delete on the same
//! URL (§2:3.104.4.3.2).

use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::patient::{Patient, PatientLink};
use fhir_types::r4::reference::Reference;
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::error::{InvalidFeed, SourceError};
use crate::pixm::identifier::SourceIdentifier;
use crate::pixm::patient::{self, REPLACED_BY};
use crate::pixm::request::{component, source_token};

/// The `Patient` type under the FHIR base.
pub(super) const PATIENT: &str = "Patient";

/// The URL of a conditional interaction on the Patient `identifier` names,
/// under the `[base]/Patient` URL `patients`: one `identifier` token whose
/// parts escape the characters FHIR search gives a meaning to.
pub(super) fn conditional(patients: &Url, identifier: &SourceIdentifier) -> Url {
    let mut url = patients.clone();
    url.set_query(Some(&format!(
        "identifier={}",
        component(source_token(identifier).expose_secret())
    )));
    url
}

/// The body of an Add or Revise Patient message about `identifier`.
pub(super) fn revision(
    identifier: &SourceIdentifier,
    patient: &Patient,
) -> Result<SecretString, SourceError> {
    if patient::replaced_by(patient).next().is_some() {
        return Err(InvalidFeed::ReplacedBy.into());
    }
    body(identifier, patient)
}

/// The body of a Resolve Duplicate Patient message: `patient`, the subsumed
/// patient, with a `replaced-by` link to `survivor` (§2:3.104.4.2.2).
pub(super) fn merge(
    subsumed: &SourceIdentifier,
    survivor: &SourceIdentifier,
    patient: &Patient,
) -> Result<SecretString, SourceError> {
    if survivor.system() != subsumed.system() {
        return Err(InvalidFeed::SurvivorInAnotherDomain.into());
    }
    if survivor.value().expose_secret() == subsumed.value().expose_secret() {
        return Err(InvalidFeed::SurvivorIsSubsumed.into());
    }
    if patient::replaced_by(patient).next().is_some() {
        return Err(InvalidFeed::ReplacedBy.into());
    }
    let mut linked = patient.clone();
    linked.link.push(PatientLink {
        other: Reference {
            identifier: Some(Box::new(Identifier {
                system: Some(survivor.system().into()),
                value: Some(survivor.value().expose_secret().into()),
                ..Identifier::default()
            })),
            ..Reference::default()
        },
        r#type: REPLACED_BY.into(),
        ..PatientLink::default()
    });
    body(subsumed, &linked)
}

/// `patient` as FHIR JSON, once it holds to the profile and carries
/// `identifier`.
fn body(identifier: &SourceIdentifier, patient: &Patient) -> Result<SecretString, SourceError> {
    if !patient::holds_to_profile(patient) {
        return Err(InvalidFeed::Profile.into());
    }
    if !patient::carries(patient, identifier) {
        return Err(InvalidFeed::IdentifierNotCarried.into());
    }
    serde_json::to_string(patient)
        .map(SecretString::from)
        .map_err(SourceError::Unwritable)
}
