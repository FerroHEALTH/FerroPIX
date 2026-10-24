// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The demographics a document shows, read from a FHIR R4 `Patient`: the
//! stored version of a record, or the one the PDQm Supplier serves.

use ferropix_operator_api::patient::{Address, Demographics, Name, Telecom};
use fhir_types::r4::patient::Patient;

use crate::feed::demographics::{self, DemographicsError};

/// Returns the demographics `patient` states.
#[must_use]
pub fn of(patient: &Patient) -> Demographics {
    Demographics {
        names: patient
            .name
            .iter()
            .map(|name| Name {
                family: name.family.as_ref().and_then(|family| family.value.clone()),
                given: name
                    .given
                    .iter()
                    .filter_map(|given| given.value.clone())
                    .collect(),
            })
            .filter(|name| name.family.is_some() || !name.given.is_empty())
            .collect(),
        birth_date: patient
            .birth_date
            .as_ref()
            .and_then(|date| date.value.clone()),
        gender: patient.gender.as_ref().and_then(|code| code.value.clone()),
        addresses: patient
            .address
            .iter()
            .map(|address| Address {
                lines: address
                    .line
                    .iter()
                    .filter_map(|line| line.value.clone())
                    .collect(),
                city: address.city.as_ref().and_then(|city| city.value.clone()),
                postal_code: address
                    .postal_code
                    .as_ref()
                    .and_then(|code| code.value.clone()),
                country: address
                    .country
                    .as_ref()
                    .and_then(|country| country.value.clone()),
            })
            .collect(),
        telecom: patient
            .telecom
            .iter()
            .filter_map(|point| {
                let value = point.value.as_ref()?.value.clone()?;
                Some(Telecom {
                    system: point.system.as_ref().and_then(|code| code.value.clone()),
                    value,
                })
            })
            .collect(),
    }
}

/// Returns the demographics of the stored JSON text `stored`.
///
/// # Errors
///
/// Returns [`DemographicsError`] when the text is no `Patient`.
pub fn stored(stored: &str) -> Result<Demographics, DemographicsError> {
    demographics::patient(stored).map(|patient| of(&patient))
}

#[cfg(test)]
mod tests {
    use super::stored;

    #[test]
    fn a_stored_version_reads_into_its_names_birth_date_gender_address_and_telecom() {
        let family = ferropix_testkit::synthetic::FAMILY_NAMES[0];
        let given = ferropix_testkit::synthetic::GIVEN_NAMES[0];
        let read = stored(&format!(
            r#"{{"resourceType":"Patient","name":[{{"family":"{family}","given":["{given}"]}}],
               "birthDate":"1970-01-01","gender":"other",
               "address":[{{"line":["Synthstreet 1"],"postalCode":"9999 ZZ"}}],
               "telecom":[{{"system":"phone","value":"+31600000001"}}]}}"#
        ))
        .expect("the version reads");
        assert_eq!(Some(family), read.names[0].family.as_deref());
        assert_eq!(vec![given.to_owned()], read.names[0].given);
        assert_eq!(Some("1970-01-01"), read.birth_date.as_deref());
        assert_eq!(Some("other"), read.gender.as_deref());
        assert_eq!(Some("9999 ZZ"), read.addresses[0].postal_code.as_deref());
        assert_eq!("+31600000001", read.telecom[0].value);
        assert!(stored("[]").is_err());
    }
}
