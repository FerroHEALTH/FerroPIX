// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The input of an ITI-119 Patient Demographics Match (§2:3.119.4.1.2).
//!
//! A Patient holding the demographics to match is posted in a `Parameters`
//! resource with the `onlyCertainMatches` and `count` parameters, as the PDQm
//! Match Input Parameters profile defines it. The Patient conforms to the
//! PDQm Patient Profile for `$match` Input, which keeps `Patient.extension`
//! open, so the birth place and the nationality ride in the core FHIR R4
//! extensions `patient-birthPlace` and `patient-nationality`.

use std::fmt;
use std::num::NonZeroU16;

use fhir_types::r4::address::Address;
use fhir_types::r4::codeable_concept::CodeableConcept;
use fhir_types::r4::coding::Coding;
use fhir_types::r4::extension::{Extension, ExtensionValue};
use fhir_types::r4::human_name::HumanName;
use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::parameters::{Parameters, ParametersParameter, ParametersParameterValue};
use fhir_types::r4::patient::Patient;
use fhir_types::r4::resource::Resource;
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::error::InvalidInput;
use super::query::fhir_date;

/// The core FHIR R4 extension holding the place of birth as an `Address`.
const BIRTH_PLACE: &str = "http://hl7.org/fhir/StructureDefinition/patient-birthPlace";

/// The core FHIR R4 extension holding a nationality, its `code` part a
/// `CodeableConcept`.
const NATIONALITY: &str = "http://hl7.org/fhir/StructureDefinition/patient-nationality";

/// The code system of ISO 3166 country codes (FHIR R4 terminologies).
const ISO_3166: &str = "urn:iso:std:iso:3166";

/// The administrative gender of the input Patient (FHIR R4
/// `administrative-gender`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    /// `male`.
    Male,
    /// `female`.
    Female,
    /// `other`.
    Other,
    /// `unknown`.
    Unknown,
}

impl Gender {
    /// The `administrative-gender` code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Male => "male",
            Self::Female => "female",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }
}

/// The place of birth of the input Patient: a locality, a region and an ISO
/// 3166-1 country code, each optional, at least one named.
///
/// The locality and the region are identifying and are never shown by
/// `Debug`.
pub struct BirthPlace {
    locality: Option<SecretString>,
    region: Option<SecretString>,
    country: Option<String>,
}

impl BirthPlace {
    /// A birth place of `locality`, `region` and `country`, as the EUDI PID
    /// `place_of_birth` names them.
    ///
    /// # Errors
    /// [`InvalidInput::Country`] when `country` is no ISO 3166-1 code, and
    /// [`InvalidInput::EmptyPlace`] when none is given or every one given is
    /// empty.
    pub fn new(
        locality: Option<&SecretString>,
        region: Option<&SecretString>,
        country: Option<&str>,
    ) -> Result<Self, InvalidInput> {
        let kept = |value: Option<&SecretString>| {
            value
                .filter(|value| !value.expose_secret().is_empty())
                .map(|value| SecretString::from(value.expose_secret()))
        };
        let country = country.map(iso_3166).transpose()?;
        let place = Self {
            locality: kept(locality),
            region: kept(region),
            country,
        };
        if place.locality.is_none() && place.region.is_none() && place.country.is_none() {
            return Err(InvalidInput::EmptyPlace);
        }
        Ok(place)
    }

    /// The place as an `Address`: the locality as `city`, the region as
    /// `state` and the country as `country`.
    fn address(&self) -> Address {
        Address {
            city: self
                .locality
                .as_ref()
                .map(|value| value.expose_secret().into()),
            state: self
                .region
                .as_ref()
                .map(|value| value.expose_secret().into()),
            country: self.country.as_deref().map(Into::into),
            ..Address::default()
        }
    }
}

impl fmt::Debug for BirthPlace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BirthPlace")
            .field("locality", &self.locality.is_some())
            .field("region", &self.region.is_some())
            .field("country", &self.country.is_some())
            .finish()
    }
}

/// The demographics one ITI-119 request asks the Supplier to match.
///
/// Every value is directly identifying and travels only in the request body
/// to the Supplier. `Debug` counts or flags each demographic and never shows
/// one, and the type has no `Display`. Each constructor names at least one
/// demographic, as the input Patient of §2:3.119.4.1.2 holds "the
/// demographic information for which the Patient Demographics Consumer
/// desires a match".
///
/// # Examples
///
/// ```
/// use ihe_iti::pdqm::input::{Gender, MatchInput};
/// use secrecy::SecretString;
///
/// let input = MatchInput::new("urn:oid:2.999.1", &SecretString::from("12345"))?
///     .only_certain_matches(true);
/// assert!(!format!("{input:?}").contains("12345"));
///
/// let named = MatchInput::named(Some(&SecretString::from("Proefsma")), &[SecretString::from("Bea")])?
///     .birth_date(&SecretString::from("1970-01-01"))?
///     .gender(Gender::Female)
///     .nationality("NL")?;
/// assert!(!format!("{named:?}").contains("Proefsma"));
/// # Ok::<(), ihe_iti::pdqm::error::InvalidInput>(())
/// ```
pub struct MatchInput {
    identifiers: Vec<(String, SecretString)>,
    family: Option<SecretString>,
    given: Vec<SecretString>,
    birth_date: Option<SecretString>,
    gender: Option<Gender>,
    birth_place: Option<BirthPlace>,
    nationalities: Vec<String>,
    only_certain_matches: Option<bool>,
    count: Option<NonZeroU16>,
}

impl MatchInput {
    /// The input with no demographic, which no constructor hands out.
    fn empty() -> Self {
        Self {
            identifiers: Vec::new(),
            family: None,
            given: Vec::new(),
            birth_date: None,
            gender: None,
            birth_place: None,
            nationalities: Vec::new(),
            only_certain_matches: None,
            count: None,
        }
    }

    /// Creates an input whose Patient carries `value` in the identifier
    /// system `system`.
    ///
    /// # Errors
    /// [`InvalidInput::System`] when `system` is not an absolute URI, and
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn new(system: &str, value: &SecretString) -> Result<Self, InvalidInput> {
        Self::empty().identifier(system, value)
    }

    /// Creates an input whose Patient is named by `family` and `given`, with
    /// no identifier.
    ///
    /// # Errors
    /// [`InvalidInput::EmptyName`] when `family` is absent or empty and every
    /// `given` name is empty or none is given.
    pub fn named(
        family: Option<&SecretString>,
        given: &[SecretString],
    ) -> Result<Self, InvalidInput> {
        Self::empty().name(family, given)
    }

    /// Creates an input whose Patient is born on `date`, with no identifier.
    ///
    /// # Errors
    /// [`InvalidInput::Date`] when `date` is not a FHIR `date`.
    pub fn born(date: &SecretString) -> Result<Self, InvalidInput> {
        Self::empty().birth_date(date)
    }

    /// Adds a `Patient.identifier` of the input Patient: `value` in the
    /// identifier system `system`.
    ///
    /// # Errors
    /// [`InvalidInput::System`] when `system` is not an absolute URI, and
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn identifier(mut self, system: &str, value: &SecretString) -> Result<Self, InvalidInput> {
        if system.is_empty()
            || system.chars().any(char::is_whitespace)
            || Url::parse(system).is_err()
        {
            return Err(InvalidInput::System);
        }
        if value.expose_secret().is_empty() {
            return Err(InvalidInput::EmptyValue);
        }
        self.identifiers
            .push((system.to_owned(), SecretString::from(value.expose_secret())));
        Ok(self)
    }

    /// Sets the `Patient.name` of the input Patient: its `family` and its
    /// `given` names, in order; an empty given name is left out.
    ///
    /// # Errors
    /// [`InvalidInput::EmptyName`] when `family` is absent or empty and every
    /// `given` name is empty or none is given.
    pub fn name(
        mut self,
        family: Option<&SecretString>,
        given: &[SecretString],
    ) -> Result<Self, InvalidInput> {
        let family = family
            .filter(|family| !family.expose_secret().is_empty())
            .map(|family| SecretString::from(family.expose_secret()));
        let given: Vec<SecretString> = given
            .iter()
            .filter(|name| !name.expose_secret().is_empty())
            .map(|name| SecretString::from(name.expose_secret()))
            .collect();
        if family.is_none() && given.is_empty() {
            return Err(InvalidInput::EmptyName);
        }
        self.family = family;
        self.given = given;
        Ok(self)
    }

    /// Sets the `Patient.birthDate` of the input Patient.
    ///
    /// # Errors
    /// [`InvalidInput::Date`] when `date` is not a FHIR `date`
    /// (<http://hl7.org/fhir/R4/datatypes.html#date>): a year, a month or a
    /// day of the calendar.
    pub fn birth_date(mut self, date: &SecretString) -> Result<Self, InvalidInput> {
        fhir_date(date)?;
        self.birth_date = Some(SecretString::from(date.expose_secret()));
        Ok(self)
    }

    /// Sets the `Patient.gender` of the input Patient.
    #[must_use]
    pub fn gender(mut self, gender: Gender) -> Self {
        self.gender = Some(gender);
        self
    }

    /// Sets the place of birth of the input Patient, written in the
    /// `patient-birthPlace` extension.
    #[must_use]
    pub fn birth_place(mut self, place: BirthPlace) -> Self {
        self.birth_place = Some(place);
        self
    }

    /// Adds a nationality of the input Patient, the ISO 3166-1 code
    /// `country`, written in a `patient-nationality` extension.
    ///
    /// # Errors
    /// [`InvalidInput::Country`] when `country` is no ISO 3166-1 code: two or
    /// three upper-case letters.
    pub fn nationality(mut self, country: &str) -> Result<Self, InvalidInput> {
        self.nationalities.push(iso_3166(country)?);
        Ok(self)
    }

    /// Sets `onlyCertainMatches`: with `true`, the Supplier returns a match
    /// only when it is certain it is the subject of the request
    /// (§2:3.119.4.1.2, Cases 3 to 5 of §2:3.119.4.1.3).
    #[must_use]
    pub fn only_certain_matches(mut self, certain: bool) -> Self {
        self.only_certain_matches = Some(certain);
        self
    }

    /// Sets `count`: the Supplier returns at most this many matches
    /// (§2:3.119.4.1.2, Case 6 of §2:3.119.4.1.3).
    #[must_use]
    pub fn count(mut self, count: NonZeroU16) -> Self {
        self.count = Some(count);
        self
    }

    /// The identifier the input names, when it names exactly one: the
    /// patient the audit record's patient entity identifies (the PDQm Match
    /// Consumer audit profile, `entity:patient`, 0..1); an input with no
    /// identifier, or with more than one, names none.
    #[cfg(feature = "balp")]
    pub(super) fn sole_identifier(&self) -> Option<(&str, &SecretString)> {
        match self.identifiers.as_slice() {
            [(system, value)] => Some((system.as_str(), value)),
            _ => None,
        }
    }

    /// The input Patient: its identifiers, name, gender, birth date, and
    /// the birth place and nationality extensions.
    fn patient(&self) -> Patient {
        let mut extension = Vec::new();
        if let Some(place) = &self.birth_place {
            extension.push(Extension {
                url: BIRTH_PLACE.to_owned(),
                value: Some(ExtensionValue::Address(Box::new(place.address()))),
                ..Extension::default()
            });
        }
        extension.extend(self.nationalities.iter().map(|country| Extension {
            url: NATIONALITY.to_owned(),
            extension: vec![Extension {
                url: "code".to_owned(),
                value: Some(ExtensionValue::CodeableConcept(Box::new(CodeableConcept {
                    coding: vec![Coding {
                        system: Some(ISO_3166.into()),
                        code: Some(country.as_str().into()),
                        ..Coding::default()
                    }],
                    ..CodeableConcept::default()
                }))),
                ..Extension::default()
            }],
            ..Extension::default()
        }));
        let name = (self.family.is_some() || !self.given.is_empty()).then(|| HumanName {
            family: self
                .family
                .as_ref()
                .map(|family| family.expose_secret().into()),
            given: self
                .given
                .iter()
                .map(|given| given.expose_secret().into())
                .collect(),
            ..HumanName::default()
        });
        Patient {
            extension,
            identifier: self
                .identifiers
                .iter()
                .map(|(system, value)| Identifier {
                    system: Some(system.as_str().into()),
                    value: Some(value.expose_secret().into()),
                    ..Identifier::default()
                })
                .collect(),
            name: name.into_iter().collect(),
            gender: self.gender.map(|gender| gender.code().into()),
            birth_date: self
                .birth_date
                .as_ref()
                .map(|date| date.expose_secret().into()),
            ..Patient::default()
        }
    }

    /// The `Parameters` resource the request posts: the input Patient in the
    /// `resource` parameter, then `onlyCertainMatches` and `count` when set
    /// (the PDQm Match Input Parameters profile).
    ///
    /// The resource holds every value, so it is handed to the HTTP client and
    /// never kept, logged or put into an error.
    pub(super) fn parameters(&self) -> Parameters {
        let mut parameter = vec![ParametersParameter {
            name: "resource".into(),
            resource: Some(Resource::Patient(Box::new(self.patient()))),
            ..ParametersParameter::default()
        }];
        if let Some(certain) = self.only_certain_matches {
            parameter.push(ParametersParameter {
                name: "onlyCertainMatches".into(),
                value: Some(ParametersParameterValue::Boolean(certain.into())),
                ..ParametersParameter::default()
            });
        }
        if let Some(count) = self.count {
            parameter.push(ParametersParameter {
                name: "count".into(),
                value: Some(ParametersParameterValue::Integer(
                    i32::from(count.get()).into(),
                )),
                ..ParametersParameter::default()
            });
        }
        Parameters {
            parameter,
            ..Parameters::default()
        }
    }
}

/// `country` when it is an ISO 3166-1 code: two (alpha-2) or three (alpha-3)
/// upper-case ASCII letters.
fn iso_3166(country: &str) -> Result<String, InvalidInput> {
    if (2..=3).contains(&country.len()) && country.bytes().all(|byte| byte.is_ascii_uppercase()) {
        Ok(country.to_owned())
    } else {
        Err(InvalidInput::Country)
    }
}

impl fmt::Debug for MatchInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MatchInput")
            .field("identifiers", &self.identifiers.len())
            .field("family", &self.family.is_some())
            .field("given", &self.given.len())
            .field("birth_date", &self.birth_date.is_some())
            .field("gender", &self.gender.is_some())
            .field("birth_place", &self.birth_place)
            .field("nationalities", &self.nationalities.len())
            .field("only_certain_matches", &self.only_certain_matches)
            .field("count", &self.count)
            .finish()
    }
}
