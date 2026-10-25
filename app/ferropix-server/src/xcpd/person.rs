// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The two crossings between an ITI-55 message and the store's `Patient`.
//!
//! [`input`] reads the query's parameters (ITI TF-2 Table
//! 3.55.4.1.2.2-1) as the `Patient` the scorer matches, as an ITI-119
//! input is read, and [`candidate`] writes a served `Patient` as the
//! `Patient` of a `RegistrationEvent` (Table 3.55.4.2.2.2-1). The codes cross
//! by HL7 v3 `AdministrativeGender` and FHIR R4 `administrative-gender`
//! (<https://hl7.org/fhir/R4/valueset-administrative-gender.html>), and a
//! `TS` by its leading digits, as the HL7 v2 face reads a `DTM`.

use fhir_types::r4::address::Address;
use fhir_types::r4::contact_point::ContactPoint;
use fhir_types::r4::extension::{Extension, ExtensionValue};
use fhir_types::r4::human_name::HumanName;
use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::patient::Patient;
use ihe_iti::xcpd::identifier::HomeCommunityId;
use ihe_iti::xcpd::responding::answer::Candidate;
use ihe_iti::xcpd::responding::query::{BirthTime, Parameters, PersonName, PostalAddress};
use secrecy::{ExposeSecret as _, SecretString};

use crate::config::section::xcpd::AnswerDomain;
use crate::hl7v2::pid::birth_date;
use crate::matching::subject::MOTHERS_MAIDEN_NAME;

/// The `URL` scheme of a telephone number (RFC 3966).
const TEL: &str = "tel:";

/// The `URL` scheme of an e-mail address (RFC 6068).
const MAILTO: &str = "mailto:";

/// Returns the `Patient` the scorer matches for `parameters`.
///
/// A birth time is read when it is one moment; a range names no birth date
/// the scorer compares, so it adds nothing. A telecom address is a
/// telephone number under `tel:` and an e-mail address under `mailto:`;
/// any other names neither.
#[must_use]
pub fn input(parameters: &Parameters) -> Patient {
    let birth = match &parameters.birth_time {
        Some(BirthTime::At(moment)) => birth_date(moment),
        Some(BirthTime::Between { .. }) | None => None,
    };
    let telecom = parameters
        .telecom
        .iter()
        .filter_map(|address| {
            let (system, value) = match address.strip_prefix(TEL) {
                Some(number) => ("phone", number),
                None => ("email", address.strip_prefix(MAILTO)?),
            };
            Some(ContactPoint {
                system: Some(system.into()),
                value: Some(value.into()),
                ..ContactPoint::default()
            })
        })
        .collect();
    let mothers_maiden_name = parameters
        .mothers_maiden_name
        .as_ref()
        .and_then(|name| joined(&name.family))
        .map(|family| Extension {
            url: MOTHERS_MAIDEN_NAME.into(),
            value: Some(ExtensionValue::String(family.as_str().into())),
            ..Extension::default()
        });
    Patient {
        extension: mothers_maiden_name.into_iter().collect(),
        name: parameters.names.iter().map(human_name).collect(),
        gender: parameters
            .gender
            .as_deref()
            .and_then(fhir_gender)
            .map(Into::into),
        birth_date: birth.map(|date| date.as_str().into()),
        address: parameters.addresses.iter().map(address).collect(),
        telecom,
        ..Patient::default()
    }
}

/// Returns the `Patient` of a `RegistrationEvent` for the served `patient`
/// in `community`.
///
/// It carries the patient's identifiers in `domains`, the first held as
/// `Patient.id` and the others as `OtherIDs`, with its names, gender, birth
/// date and addresses.
///
/// `None` when the patient holds no identifier in `domains`, since the
/// answer would name no patient the initiating community can query for.
#[must_use]
pub fn candidate(
    patient: &Patient,
    community: &HomeCommunityId,
    domains: &[AnswerDomain],
) -> Option<Candidate> {
    let mut held = domains.iter().flat_map(|domain| {
        patient
            .identifier
            .iter()
            .filter(|identifier| system(identifier) == Some(domain.system.as_str()))
            .filter_map(|identifier| {
                let value = identifier.value.as_ref()?.value.as_deref()?;
                Some((
                    domain.authority.clone(),
                    SecretString::from(value.to_owned()),
                ))
            })
    });
    let patient_id = held.next()?;
    let other_ids = held.collect();
    Some(Candidate {
        community: community.clone(),
        patient_id,
        other_ids,
        names: patient.name.iter().filter_map(person_name).collect(),
        gender: patient
            .gender
            .as_ref()
            .and_then(|gender| gender.value.as_deref())
            .and_then(v3_gender)
            .map(str::to_owned),
        birth_time: patient
            .birth_date
            .as_ref()
            .and_then(|date| date.value.as_deref())
            .map(|date| date.chars().filter(char::is_ascii_digit).collect()),
        addresses: patient.address.iter().map(postal_address).collect(),
    })
}

/// Returns the identifier's system.
fn system(identifier: &Identifier) -> Option<&str> {
    identifier.system.as_ref()?.value.as_deref()
}

/// Returns `parts` joined by a space, `None` when there are none.
fn joined(parts: &[String]) -> Option<String> {
    let text = parts.join(" ");
    (!text.trim().is_empty()).then_some(text)
}

/// Returns the FHIR `HumanName` of a `PN`.
fn human_name(name: &PersonName) -> HumanName {
    HumanName {
        family: joined(&name.family).map(|family| family.as_str().into()),
        given: name
            .given
            .iter()
            .map(|given| given.as_str().into())
            .collect(),
        ..HumanName::default()
    }
}

/// Returns the `PN` of a FHIR `HumanName`, `None` when it states no part.
fn person_name(name: &HumanName) -> Option<PersonName> {
    let family: Vec<String> = name
        .family
        .as_ref()
        .and_then(|family| family.value.clone())
        .into_iter()
        .collect();
    let given: Vec<String> = name
        .given
        .iter()
        .filter_map(|given| given.value.clone())
        .collect();
    (!family.is_empty() || !given.is_empty()).then_some(PersonName { given, family })
}

/// Returns the FHIR `Address` of an `AD`.
fn address(address: &PostalAddress) -> Address {
    Address {
        line: address
            .lines
            .iter()
            .map(|line| line.as_str().into())
            .collect(),
        city: address.city.as_deref().map(Into::into),
        state: address.state.as_deref().map(Into::into),
        postal_code: address.postal_code.as_deref().map(Into::into),
        country: address.country.as_deref().map(Into::into),
        ..Address::default()
    }
}

/// Returns the `AD` of a FHIR `Address`.
fn postal_address(address: &Address) -> PostalAddress {
    let text = |part: Option<&fhir_types::r4::primitives::String>| {
        part.and_then(|part| part.value.clone())
    };
    PostalAddress {
        lines: address
            .line
            .iter()
            .filter_map(|line| line.value.clone())
            .collect(),
        city: text(address.city.as_ref()),
        state: text(address.state.as_ref()),
        postal_code: text(address.postal_code.as_ref()),
        country: text(address.country.as_ref()),
    }
}

/// Returns the FHIR `administrative-gender` of an HL7 v3
/// `AdministrativeGender` code: `M`, `F` and `UN`.
fn fhir_gender(code: &str) -> Option<&'static str> {
    match code {
        "M" => Some("male"),
        "F" => Some("female"),
        "UN" => Some("other"),
        _ => None,
    }
}

/// Returns the HL7 v3 `AdministrativeGender` code of a FHIR
/// `administrative-gender` code; `unknown` has none.
fn v3_gender(code: &str) -> Option<&'static str> {
    match code {
        "male" => Some("M"),
        "female" => Some("F"),
        "other" => Some("UN"),
        _ => None,
    }
}

/// Returns the identifier value of a candidate's `Patient.id`, for the
/// audit record.
#[must_use]
pub fn patient_id(candidate: &Candidate) -> (String, SecretString) {
    let (authority, value) = &candidate.patient_id;
    (
        authority.urn(),
        SecretString::from(value.expose_secret().to_owned()),
    )
}

#[cfg(test)]
mod tests {
    use fhir_types::r4::identifier::Identifier;
    use fhir_types::r4::patient::Patient;
    use ihe_iti::xcpd::identifier::{HomeCommunityId, Oid};
    use ihe_iti::xcpd::responding::query::{BirthTime, Parameters, PersonName, PostalAddress};
    use secrecy::ExposeSecret as _;

    use super::{candidate, input};
    use crate::config::section::xcpd::AnswerDomain;

    fn domain(oid: &str) -> AnswerDomain {
        let authority = Oid::new(oid).expect("an OID");
        AnswerDomain {
            system: authority.urn(),
            authority,
        }
    }

    fn identifier(system: &str, value: &str) -> Identifier {
        Identifier {
            system: Some(system.into()),
            value: Some(value.into()),
            ..Identifier::default()
        }
    }

    #[test]
    fn the_query_reads_as_the_patient_the_scorer_matches() {
        let parameters = Parameters {
            names: vec![PersonName {
                given: vec![String::from("Anna")],
                family: vec![String::from("van"), String::from("Dijk")],
            }],
            gender: Some(String::from("F")),
            birth_time: Some(BirthTime::At(String::from("19800412"))),
            addresses: vec![PostalAddress {
                lines: vec![String::from("Kerkstraat 1")],
                city: Some(String::from("Utrecht")),
                postal_code: Some(String::from("3511AA")),
                ..PostalAddress::default()
            }],
            telecom: vec![
                String::from("tel:+31301234567"),
                String::from("mailto:anna@example.org"),
                String::from("fax:+31301234568"),
            ],
            mothers_maiden_name: Some(PersonName {
                given: Vec::new(),
                family: vec![String::from("Jansen")],
            }),
            ..Parameters::default()
        };
        let patient = input(&parameters);
        assert_eq!(
            Some("van Dijk"),
            patient.name[0]
                .family
                .as_ref()
                .and_then(|f| f.value.as_deref())
        );
        assert_eq!(
            Some("female"),
            patient.gender.as_ref().and_then(|g| g.value.as_deref())
        );
        assert_eq!(
            Some("1980-04-12"),
            patient.birth_date.as_ref().and_then(|d| d.value.as_deref())
        );
        assert_eq!(2, patient.telecom.len());
        assert_eq!(1, patient.extension.len());
        assert_eq!(
            Some("3511AA"),
            patient.address[0]
                .postal_code
                .as_ref()
                .and_then(|p| p.value.as_deref())
        );
        let range = Parameters {
            birth_time: Some(BirthTime::Between {
                low: Some(String::from("1980")),
                high: None,
            }),
            ..Parameters::default()
        };
        assert!(input(&range).birth_date.is_none());
    }

    #[test]
    fn a_served_patient_answers_with_its_first_domain_as_patient_id() {
        let community = HomeCommunityId::new(Oid::new("2.999.7").expect("an OID"));
        let domains = [domain("2.999.1.2"), domain("2.999.1.1")];
        let patient = Patient {
            identifier: vec![
                identifier("https://ferropix.example.org", "person"),
                identifier("urn:oid:2.999.1.1", "H-1"),
                identifier("urn:oid:2.999.1.2", "L-1"),
                identifier("urn:oid:2.999.1.3", "P-1"),
            ],
            gender: Some("male".into()),
            birth_date: Some("1975-01-31".into()),
            ..Patient::default()
        };
        let answered = candidate(&patient, &community, &domains).expect("a candidate");
        assert_eq!("2.999.1.2", answered.patient_id.0.as_str());
        assert_eq!("L-1", answered.patient_id.1.expose_secret());
        assert_eq!(1, answered.other_ids.len());
        assert_eq!("2.999.1.1", answered.other_ids[0].0.as_str());
        assert_eq!(Some("M"), answered.gender.as_deref());
        assert_eq!(Some("19750131"), answered.birth_time.as_deref());
        let elsewhere = Patient {
            identifier: vec![identifier("urn:oid:2.999.1.3", "P-1")],
            ..Patient::default()
        };
        assert!(candidate(&elsewhere, &community, &domains).is_none());
    }
}
