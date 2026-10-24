// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The demographics of one record version as the scorer compares them.
//!
//! A [`Subject`] is read from the `Patient` a feed carried, the elements of
//! the PDQm Patient profile the Manager accepts (PIXm 3.1.0
//! §2:3.104.4.1.2.1): the names, the birth date, the administrative gender,
//! the addresses, the telecom and the mother's maiden name extension, each
//! in the form [`crate::matching::normalise`] gives it, beside
//! the identifiers the record's version carries in `local` domains as
//! [`IdentifierKey`]s. No specification governs the reading: our own design
//! (`docs/architecture.md` §5.2).

use std::collections::BTreeSet;
use std::fmt;

use fhir_types::r4::extension::ExtensionValue;
use fhir_types::r4::human_name::HumanName;
use fhir_types::r4::patient::Patient;
use uuid::Uuid;

use crate::matching::normalise;

/// The extension that carries the mother's maiden name, which PDQm searches
/// on (PDQm 3.2.0 §2:3.78.4.1.2.1, `mothersMaidenName`).
pub const MOTHERS_MAIDEN_NAME: &str =
    "http://hl7.org/fhir/StructureDefinition/patient-mothersMaidenName";

/// An identifier as the scorer compares it: its domain and the HMAC of its
/// normalised value (`docs/architecture.md` §4.2), never the value.
///
/// `Debug` shows the domain alone.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdentifierKey {
    /// The domain's id.
    domain: Uuid,
    /// The HMAC of the domain id and the normalised value.
    hmac: [u8; 32],
}

impl IdentifierKey {
    /// Returns the key of the value whose HMAC is `hmac` in `domain`.
    #[must_use]
    pub const fn new(domain: Uuid, hmac: [u8; 32]) -> Self {
        Self { domain, hmac }
    }

    /// Returns the domain's id.
    #[must_use]
    pub const fn domain(&self) -> Uuid {
        self.domain
    }

    /// Returns the HMAC.
    #[must_use]
    pub const fn hmac(&self) -> &[u8; 32] {
        &self.hmac
    }
}

impl fmt::Debug for IdentifierKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdentifierKey")
            .field("domain", &self.domain)
            .finish_non_exhaustive()
    }
}

/// A birth date, to the precision the record states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Birth {
    /// The year.
    pub year: u16,
    /// The month and the day, when the date is stated to the day.
    pub day: Option<(u8, u8)>,
}

impl Birth {
    /// Reads a FHIR R4 `date`: `YYYY`, `YYYY-MM` or `YYYY-MM-DD`; a month
    /// alone keeps only the year.
    #[must_use]
    pub fn parse(date: &str) -> Option<Self> {
        let mut parts = date.split('-');
        let year = parts.next().filter(|y| y.len() == 4)?.parse().ok()?;
        let month = parts.next();
        let day = parts.next();
        let day = match (month, day) {
            (Some(month), Some(day)) => Some((month.parse().ok()?, day.parse().ok()?)),
            _ => None,
        };
        Some(Self { year, day })
    }

    /// Returns the date as `YYYYMMDD` when it is stated to the day.
    #[must_use]
    pub fn digits(self) -> Option<String> {
        self.day
            .map(|(month, day)| format!("{:04}{month:02}{day:02}", self.year))
    }
}

/// One name: the family name and the given names, normalised.
///
/// `Debug` shows which parts are present, never a value.
#[derive(Clone, PartialEq, Eq)]
pub struct Name {
    /// The family name.
    pub family: Option<String>,
    /// The given names, in order.
    pub given: Vec<String>,
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Name")
            .field("family", &self.family.is_some())
            .field("given", &self.given.len())
            .finish()
    }
}

/// The demographics of one record version, normalised for the scorer.
///
/// `Debug` shows which fields are present, never a value.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Subject {
    /// The names, the official and usual ones first.
    pub names: Vec<Name>,
    /// The birth date.
    pub birth: Option<Birth>,
    /// The administrative gender, `unknown` read as absent.
    pub gender: Option<String>,
    /// The postal codes, the home address first.
    pub postal_codes: Vec<String>,
    /// The words of each address's lines and city.
    pub addresses: Vec<BTreeSet<String>>,
    /// The telephone numbers.
    pub telephones: Vec<String>,
    /// The email addresses.
    pub emails: Vec<String>,
    /// The mother's maiden name.
    pub mothers_maiden_name: Option<String>,
    /// The identifiers the version carries in `local` domains.
    pub local_identifiers: BTreeSet<IdentifierKey>,
}

impl Subject {
    /// Reads the subject of `patient`, with `local_identifiers` the keys of
    /// the identifiers it carries in `local` domains.
    #[must_use]
    pub fn from_patient(patient: &Patient, local_identifiers: BTreeSet<IdentifierKey>) -> Self {
        let mut names: Vec<(u8, Name)> = patient
            .name
            .iter()
            .filter_map(|name| Some((name_rank(name), read_name(name)?)))
            .collect();
        names.sort_by_key(|(rank, _)| *rank);
        let mut addresses: Vec<_> = patient
            .address
            .iter()
            .map(|address| {
                let rank = u8::from(code(address.r#use.as_ref()) != Some("home"));
                (rank, address)
            })
            .collect();
        addresses.sort_by_key(|(rank, _)| *rank);
        let postal_codes = addresses
            .iter()
            .filter_map(|(_, address)| {
                normalise::postal_code(string(address.postal_code.as_ref())?)
            })
            .collect();
        let addresses = addresses
            .iter()
            .map(|(_, address)| {
                address
                    .line
                    .iter()
                    .chain(address.city.as_ref())
                    .filter_map(|part| part.value.as_deref())
                    .flat_map(normalise::words)
                    .collect::<BTreeSet<_>>()
            })
            .filter(|words| !words.is_empty())
            .collect();
        let telecom = |system: &str, read: fn(&str) -> Option<String>| {
            patient
                .telecom
                .iter()
                .filter(|point| code(point.system.as_ref()) == Some(system))
                .filter_map(|point| read(string(point.value.as_ref())?))
                .collect::<Vec<_>>()
        };
        let mothers_maiden_name = patient
            .extension
            .iter()
            .filter(|extension| extension.url == MOTHERS_MAIDEN_NAME)
            .find_map(|extension| match &extension.value {
                Some(ExtensionValue::String(text)) => normalise::text(text.value.as_deref()?),
                _ => None,
            });
        Self {
            names: names.into_iter().map(|(_, name)| name).collect(),
            birth: patient
                .birth_date
                .as_ref()
                .and_then(|date| date.value.as_deref())
                .and_then(Birth::parse),
            gender: code(patient.gender.as_ref())
                .filter(|gender| *gender != "unknown")
                .map(str::to_owned),
            postal_codes,
            addresses,
            telephones: telecom("phone", normalise::telephone),
            emails: telecom("email", normalise::email),
            mothers_maiden_name,
            local_identifiers,
        }
    }

    /// Returns the first family name, the one the blocking key is made of.
    #[must_use]
    pub fn family(&self) -> Option<&str> {
        self.names.iter().find_map(|name| name.family.as_deref())
    }

    /// Returns how many of the scorer's fields the subject states, each
    /// counted once however many values it holds.
    #[must_use]
    pub fn stated(&self) -> usize {
        [
            self.family().is_some(),
            self.names.iter().any(|name| !name.given.is_empty()),
            self.birth.is_some(),
            self.gender.is_some(),
            !self.postal_codes.is_empty(),
            !self.addresses.is_empty(),
            !self.telephones.is_empty(),
            !self.emails.is_empty(),
            self.mothers_maiden_name.is_some(),
            !self.local_identifiers.is_empty(),
        ]
        .into_iter()
        .filter(|stated| *stated)
        .count()
    }
}

impl fmt::Debug for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subject")
            .field("names", &self.names.len())
            .field("birth", &self.birth.is_some())
            .field("gender", &self.gender.is_some())
            .field("postal_codes", &self.postal_codes.len())
            .field("addresses", &self.addresses.len())
            .field("telephones", &self.telephones.len())
            .field("emails", &self.emails.len())
            .field("mothers_maiden_name", &self.mothers_maiden_name.is_some())
            .field("local_identifiers", &self.local_identifiers.len())
            .finish()
    }
}

/// The order a name is compared in: official, usual, then the rest.
fn name_rank(name: &HumanName) -> u8 {
    match code(name.r#use.as_ref()) {
        Some("official") => 0,
        Some("usual") => 1,
        _ => 2,
    }
}

/// Reads one name, or `None` when it holds neither a family nor a given name.
fn read_name(name: &HumanName) -> Option<Name> {
    let family = string(name.family.as_ref()).and_then(normalise::text);
    let given: Vec<String> = name
        .given
        .iter()
        .filter_map(|given| normalise::text(given.value.as_deref()?))
        .collect();
    (family.is_some() || !given.is_empty()).then_some(Name { family, given })
}

/// The value of a `code`.
fn code(code: Option<&fhir_types::r4::primitives::Code>) -> Option<&str> {
    code.and_then(|code| code.value.as_deref())
}

/// The value of a `string`.
fn string(text: Option<&fhir_types::r4::primitives::String>) -> Option<&str> {
    text.and_then(|text| text.value.as_deref())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES};
    use fhir_types::codec::{Json as _, Path, Value};
    use fhir_types::r4::patient::Patient;

    use super::{Birth, Subject};

    fn patient(json: &str) -> Patient {
        let value: Value = serde_json::from_str(json).unwrap();
        Patient::from_json(value.as_object().unwrap(), &mut Path::root("Patient")).unwrap()
    }

    #[test]
    fn a_patient_reads_into_its_normalised_subject() {
        let family = FAMILY_NAMES[0];
        let given = GIVEN_NAMES[0];
        let mother = FAMILY_NAMES[1];
        let subject = Subject::from_patient(
            &patient(&format!(
                r#"{{"resourceType":"Patient",
                "name":[{{"use":"maiden","family":"X{family}"}},{{"use":"official","family":"van {family}","given":["{given}","B."]}}],
                "birthDate":"1958-03-14","gender":"female",
                "address":[{{"use":"work","postalCode":"9999 zz"}},{{"use":"home","line":["Kade 1"],"city":"Voorbeeld","postalCode":"1234 ab"}}],
                "telecom":[{{"system":"phone","value":"06-12345678"}},{{"system":"email","value":"A@Example.org"}}],
                "extension":[{{"url":"http://hl7.org/fhir/StructureDefinition/patient-mothersMaidenName","valueString":"{mother}"}}]}}"#
            )),
            BTreeSet::new(),
        );
        let expected_family = format!("van {}", family.to_lowercase());
        assert_eq!(Some(expected_family.as_str()), subject.family());
        assert_eq!(
            vec![given.to_lowercase(), String::from("b")],
            subject.names[0].given
        );
        assert_eq!(
            Some(Birth {
                year: 1958,
                day: Some((3, 14))
            }),
            subject.birth
        );
        assert_eq!(Some("female"), subject.gender.as_deref());
        assert_eq!(vec!["1234AB", "9999ZZ"], subject.postal_codes);
        assert_eq!(vec!["612345678"], subject.telephones);
        assert_eq!(vec!["a@example.org"], subject.emails);
        assert_eq!(Some(mother.to_lowercase()), subject.mothers_maiden_name);
        assert_eq!(9, subject.stated(), "every field but a local identifier");
        assert_eq!(0, Subject::default().stated());
        let home: BTreeSet<String> = ["kade", "1", "voorbeeld"].map(String::from).into();
        assert_eq!(vec![home], subject.addresses);
    }

    #[test]
    fn an_unknown_gender_and_a_date_without_a_day_read_as_absent() {
        let subject = Subject::from_patient(
            &patient(r#"{"resourceType":"Patient","gender":"unknown","birthDate":"1958-03"}"#),
            BTreeSet::new(),
        );
        assert_eq!(None, subject.gender);
        assert_eq!(
            Some(Birth {
                year: 1958,
                day: None
            }),
            subject.birth
        );
        assert_eq!(None, subject.birth.and_then(Birth::digits));
    }

    #[test]
    fn the_debug_of_a_subject_names_no_value() {
        let family = FAMILY_NAMES[2];
        let subject = Subject::from_patient(
            &patient(&format!(
                r#"{{"resourceType":"Patient","name":[{{"family":"{family}"}}]}}"#
            )),
            BTreeSet::new(),
        );
        let rendered = format!("{subject:?}");
        assert!(
            !rendered.to_lowercase().contains(&family.to_lowercase()),
            "{rendered}"
        );
    }
}
