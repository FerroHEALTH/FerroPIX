// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The demographics a version stores: the fed `Patient` without a value
//! that names the patient in a domain.
//!
//! The identifiers go to their own rows, found by their HMAC and sealed in
//! a sensitive domain (`docs/architecture.md` §4.2), so the `jsonb` column
//! holds no identifier: `identifier`, `link` (whose `other.identifier`
//! names the survivor of a duplicate), `contained`, the narrative `text`
//! and the resource's `id` and `meta` are dropped, and the elements of the
//! PDQm Patient profile the scorer and the search read stay (PIXm 3.1.0
//! §2:3.104.4.1.2.1). No specification governs what is stored: our own
//! design.

use fhir_types::codec::{Json as _, Path, Value};
use fhir_types::r4::patient::Patient;

use crate::store::record::Demographics;

/// The members of a `Patient` the stored demographics leave out.
const DROPPED: [&str; 6] = ["id", "meta", "text", "contained", "identifier", "link"];

/// Returns the demographics `patient` stores.
///
/// # Errors
/// Returns [`DemographicsError`] when the generated model cannot write the
/// Patient, which a Patient it read never causes.
pub fn stored(patient: &Patient) -> Result<Demographics, DemographicsError> {
    let mut object = patient.to_json().map_err(|_encode| DemographicsError)?;
    for member in DROPPED {
        object.remove(member);
    }
    let text = serde_json::to_string(&Value::Object(object)).map_err(|_json| DemographicsError)?;
    Demographics::from_json(text).map_err(|_json| DemographicsError)
}

/// Reads the `Patient` a version's stored demographics hold.
///
/// # Errors
/// Returns [`DemographicsError`] when the stored text is no Patient the
/// generated model reads.
pub fn patient(demographics: &str) -> Result<Patient, DemographicsError> {
    let value: Value = serde_json::from_str(demographics).map_err(|_json| DemographicsError)?;
    let object = value.as_object().ok_or(DemographicsError)?;
    Patient::from_json(object, &mut Path::root("Patient")).map_err(|_decode| DemographicsError)
}

/// Stored demographics that do not read, or a Patient that does not write.
///
/// It carries neither the text nor the parser's message, which may quote a
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the stored demographics are no Patient the generated model reads or writes")]
pub struct DemographicsError;

#[cfg(test)]
mod tests {
    use ferropix_testkit::synthetic::{FAMILY_NAMES, HOSPITAL_DOMAIN};

    use super::{patient, stored};

    #[test]
    fn the_stored_demographics_hold_no_identifier_and_read_back_as_the_patient() {
        let family = FAMILY_NAMES[0];
        let fed = patient(&format!(
            r#"{{"resourceType":"Patient","id":"x","identifier":[{{"system":"{HOSPITAL_DOMAIN}","value":"H-777001"}}],
            "name":[{{"family":"{family}"}}],"birthDate":"1961-04-12",
            "link":[{{"other":{{"identifier":{{"system":"{HOSPITAL_DOMAIN}","value":"H-777002"}}}},"type":"replaced-by"}}]}}"#
        ))
        .unwrap();
        let demographics = stored(&fed).unwrap();
        let text = demographics.expose();
        assert!(!text.contains("H-777001"), "the identifier is dropped");
        assert!(
            !text.contains("H-777002"),
            "the survivor's identifier is dropped"
        );
        assert!(!text.contains("\"id\""), "the logical id is dropped");
        let back = patient(text).unwrap();
        assert!(back.identifier.is_empty() && back.link.is_empty());
        assert_eq!(
            Some(family),
            back.name[0]
                .family
                .as_ref()
                .and_then(|f| f.value.as_deref())
        );
    }

    #[test]
    fn text_that_is_no_patient_is_refused_without_quoting_it() {
        let error = patient(r#"{"resourceType":"Patient","birthDate":17}"#).unwrap_err();
        assert!(!error.to_string().contains("17"));
        assert!(patient("[").is_err());
    }
}
