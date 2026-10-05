// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The demographic criteria of an ITI-78 search: the Patient search
//! parameters a Supplier processes (§2:3.78.4.1.2.1) and the `:exact`
//! modifier it supports on each string parameter (§2:3.78.4.1.2.2).

use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::search::escape;

use super::error::InvalidInput;

/// How a string parameter matches (FHIR R4 search, `string`,
/// <http://hl7.org/fhir/R4/search.html#string>).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum StringMatch {
    /// The field starts with the value, ignoring case and accents: FHIR's
    /// default for a string parameter.
    #[default]
    StartsWith,
    /// The field equals the value exactly: the `:exact` modifier.
    Exact,
}

/// The comparison of a `birthdate` value (FHIR R4 search, prefixes,
/// <http://hl7.org/fhir/R4/search.html#prefix>).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DatePrefix {
    /// `eq`: the birth date lies within the value's precision.
    #[default]
    Eq,
    /// `ne`
    Ne,
    /// `gt`
    Gt,
    /// `lt`
    Lt,
    /// `ge`
    Ge,
    /// `le`
    Le,
    /// `sa`: the birth date starts after the value.
    Sa,
    /// `eb`: the birth date ends before the value.
    Eb,
    /// `ap`: the birth date is approximately the value.
    Ap,
}

impl DatePrefix {
    fn code(self) -> &'static str {
        match self {
            Self::Eq => "",
            Self::Ne => "ne",
            Self::Gt => "gt",
            Self::Lt => "lt",
            Self::Ge => "ge",
            Self::Le => "le",
            Self::Sa => "sa",
            Self::Eb => "eb",
            Self::Ap => "ap",
        }
    }
}

/// The administrative gender a `gender` parameter asks for (FHIR R4
/// `administrative-gender`, a required binding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Gender {
    /// `male`
    Male,
    /// `female`
    Female,
    /// `other`
    Other,
    /// `unknown`
    Unknown,
}

impl Gender {
    fn code(self) -> &'static str {
        match self {
            Self::Male => "male",
            Self::Female => "female",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }
}

/// One search parameter: its name, with any modifier, and its encoded value.
struct Parameter {
    name: &'static str,
    value: SecretString,
}

/// The criteria of one ITI-78 search, every parameter joined by AND.
///
/// Every value is directly identifying and travels only in the request body
/// to the Supplier. `Debug` names the parameters and never a value, and the
/// type has no `Display`.
///
/// # Examples
///
/// ```
/// use ihe_iti::pdqm::query::{DatePrefix, Gender, PatientQuery, StringMatch};
/// use secrecy::SecretString;
///
/// let query = PatientQuery::new()
///     .family(&SecretString::from("Schmidt"), StringMatch::Exact)?
///     .birthdate(DatePrefix::Eq, &SecretString::from("1923-07-25"))?
///     .gender(Gender::Male);
/// let names: Vec<&str> = query.names().collect();
/// assert_eq!(names, ["family:exact", "birthdate", "gender"]);
/// # Ok::<(), ihe_iti::pdqm::error::InvalidInput>(())
/// ```
#[derive(Default)]
pub struct PatientQuery {
    parameters: Vec<Parameter>,
}

impl PatientQuery {
    /// Creates a query with no criteria.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `_id`: the Patient's logical id on the Supplier.
    ///
    /// # Errors
    /// [`InvalidInput::Id`] when `id` is not a FHIR `id`
    /// (<http://hl7.org/fhir/R4/datatypes.html#id>).
    pub fn id(self, id: &SecretString) -> Result<Self, InvalidInput> {
        let valid = {
            let text = id.expose_secret();
            (1..=64).contains(&text.len())
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
        };
        if !valid {
            return Err(InvalidInput::Id);
        }
        Ok(self.with("_id", SecretString::from(id.expose_secret())))
    }

    /// Adds `active`: whether the Patient record is in active use.
    #[must_use]
    pub fn active(self, active: bool) -> Self {
        let value = if active { "true" } else { "false" };
        self.with("active", SecretString::from(value))
    }

    /// Adds `family`.
    ///
    /// # Errors
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn family(self, value: &SecretString, matching: StringMatch) -> Result<Self, InvalidInput> {
        self.string(["family", "family:exact"], value, matching)
    }

    /// Adds `given`.
    ///
    /// # Errors
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn given(self, value: &SecretString, matching: StringMatch) -> Result<Self, InvalidInput> {
        self.string(["given", "given:exact"], value, matching)
    }

    /// Adds `identifier` as `system|value`, or as the bare `value` in any
    /// system when `system` is `None`.
    ///
    /// # Errors
    /// [`InvalidInput::System`] when `system` is not an absolute URI, and
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn identifier(
        self,
        system: Option<&str>,
        value: &SecretString,
    ) -> Result<Self, InvalidInput> {
        let value = non_empty(value)?;
        let token = match system {
            Some(system) => format!("{}|{}", escape(absolute_uri(system)?), escape(value)),
            None => escape(value),
        };
        Ok(self.with("identifier", SecretString::from(token)))
    }

    /// Adds the identifier domains whose identifiers the matched Patients
    /// carry, any one of them: `identifier=system1|,system2|`
    /// (§2:3.78.4.1.2.3).
    ///
    /// # Errors
    /// [`InvalidInput::System`] when a system is not an absolute URI, and
    /// [`InvalidInput::EmptyValue`] when `systems` is empty.
    pub fn identifier_domains(self, systems: &[&str]) -> Result<Self, InvalidInput> {
        if systems.is_empty() {
            return Err(InvalidInput::EmptyValue);
        }
        let mut domains = Vec::with_capacity(systems.len());
        for system in systems {
            domains.push(format!("{}|", escape(absolute_uri(system)?)));
        }
        Ok(self.with("identifier", SecretString::from(domains.join(","))))
    }

    /// Adds `telecom`: a phone number, an e-mail address or another contact
    /// point value.
    ///
    /// # Errors
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn telecom(self, value: &SecretString) -> Result<Self, InvalidInput> {
        let token = escape(non_empty(value)?);
        Ok(self.with("telecom", SecretString::from(token)))
    }

    /// Adds `birthdate`, compared to `date` by `prefix`.
    ///
    /// # Errors
    /// [`InvalidInput::Date`] when `date` is not a FHIR `date`
    /// (<http://hl7.org/fhir/R4/datatypes.html#date>).
    pub fn birthdate(self, prefix: DatePrefix, date: &SecretString) -> Result<Self, InvalidInput> {
        if !fhir_date(date.expose_secret()) {
            return Err(InvalidInput::Date);
        }
        let value = format!("{}{}", prefix.code(), date.expose_secret());
        Ok(self.with("birthdate", SecretString::from(value)))
    }

    /// Adds `address`: any part of the address.
    ///
    /// # Errors
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn address(
        self,
        value: &SecretString,
        matching: StringMatch,
    ) -> Result<Self, InvalidInput> {
        self.string(["address", "address:exact"], value, matching)
    }

    /// Adds `address-city`.
    ///
    /// # Errors
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn address_city(
        self,
        value: &SecretString,
        matching: StringMatch,
    ) -> Result<Self, InvalidInput> {
        self.string(["address-city", "address-city:exact"], value, matching)
    }

    /// Adds `address-country`.
    ///
    /// # Errors
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn address_country(
        self,
        value: &SecretString,
        matching: StringMatch,
    ) -> Result<Self, InvalidInput> {
        self.string(
            ["address-country", "address-country:exact"],
            value,
            matching,
        )
    }

    /// Adds `address-postalcode`.
    ///
    /// # Errors
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn address_postalcode(
        self,
        value: &SecretString,
        matching: StringMatch,
    ) -> Result<Self, InvalidInput> {
        self.string(
            ["address-postalcode", "address-postalcode:exact"],
            value,
            matching,
        )
    }

    /// Adds `address-state`.
    ///
    /// # Errors
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn address_state(
        self,
        value: &SecretString,
        matching: StringMatch,
    ) -> Result<Self, InvalidInput> {
        self.string(["address-state", "address-state:exact"], value, matching)
    }

    /// Adds `gender`.
    #[must_use]
    pub fn gender(self, gender: Gender) -> Self {
        self.with("gender", SecretString::from(gender.code()))
    }

    /// Adds `mothersMaidenName`, the PDQm search parameter on the
    /// `patient-mothersMaidenName` extension.
    ///
    /// # Errors
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn mothers_maiden_name(
        self,
        value: &SecretString,
        matching: StringMatch,
    ) -> Result<Self, InvalidInput> {
        self.string(
            ["mothersMaidenName", "mothersMaidenName:exact"],
            value,
            matching,
        )
    }

    /// Returns the names of the parameters, with their modifiers, in order.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.parameters.iter().map(|parameter| parameter.name)
    }

    /// Whether the query names an identifier domain, the one case in which a
    /// `404` means "domain not recognized" (§2:3.78.4.1.3, Case 4).
    pub(super) fn names_domain(&self) -> bool {
        self.parameters.iter().any(|parameter| {
            parameter.name == "identifier" && parameter.value.expose_secret().contains('|')
        })
    }

    /// The `application/x-www-form-urlencoded` body of the `POST` search
    /// (FHIR R4 search, <http://hl7.org/fhir/R4/http.html#search>).
    ///
    /// The body holds every value, so it is handed to the HTTP client and
    /// never kept, logged or put into an error.
    pub(super) fn form(&self) -> String {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        for parameter in &self.parameters {
            form.append_pair(parameter.name, parameter.value.expose_secret());
        }
        form.finish()
    }

    fn string(
        self,
        [name, exact]: [&'static str; 2],
        value: &SecretString,
        matching: StringMatch,
    ) -> Result<Self, InvalidInput> {
        let name = match matching {
            StringMatch::StartsWith => name,
            StringMatch::Exact => exact,
        };
        let value = escape(non_empty(value)?);
        Ok(self.with(name, SecretString::from(value)))
    }

    fn with(mut self, name: &'static str, value: SecretString) -> Self {
        self.parameters.push(Parameter { name, value });
        self
    }
}

impl fmt::Debug for PatientQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.names().collect();
        f.debug_struct("PatientQuery")
            .field("parameters", &names)
            .finish()
    }
}

fn non_empty(value: &SecretString) -> Result<&str, InvalidInput> {
    let text = value.expose_secret();
    if text.is_empty() {
        Err(InvalidInput::EmptyValue)
    } else {
        Ok(text)
    }
}

fn absolute_uri(text: &str) -> Result<&str, InvalidInput> {
    if text.is_empty() || text.chars().any(char::is_whitespace) || Url::parse(text).is_err() {
        return Err(InvalidInput::System);
    }
    Ok(text)
}

/// Whether `text` is a FHIR `date`: `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, with a
/// month of 01 to 12 and a day of 01 to 31
/// (<http://hl7.org/fhir/R4/datatypes.html#date>).
fn fhir_date(text: &str) -> bool {
    let mut parts = text.split('-');
    let year = parts.next();
    let month = parts.next();
    let day = parts.next();
    if parts.next().is_some() || !year.is_some_and(|year| digits(year, 4)) {
        return false;
    }
    let in_range = |part: &str, high: u8| {
        digits(part, 2)
            && part
                .parse::<u8>()
                .is_ok_and(|number| (1..=high).contains(&number))
    };
    match (month, day) {
        (None, None) => true,
        (Some(month), None) => in_range(month, 12),
        (Some(month), Some(day)) => in_range(month, 12) && in_range(day, 31),
        (None, Some(_)) => false,
    }
}

fn digits(part: &str, length: usize) -> bool {
    part.len() == length && part.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::{DatePrefix, Gender, InvalidInput, PatientQuery, StringMatch, fhir_date};

    fn secret(text: &str) -> SecretString {
        SecretString::from(text)
    }

    #[test]
    fn a_fhir_date_has_a_year_and_optionally_a_month_and_a_day() {
        for date in ["1923", "1923-07", "1923-07-25", "2000-12-31"] {
            assert!(fhir_date(date), "{date} is a FHIR date");
        }
        for date in [
            "",
            "23",
            "1923-7",
            "1923-13",
            "1923-00",
            "1923-07-32",
            "1923-07-25T00:00",
            "1923--25",
            "1923-07-25-01",
            "abcd",
        ] {
            assert!(!fhir_date(date), "{date} is not a FHIR date");
        }
    }

    #[test]
    fn the_form_carries_each_parameter_with_its_modifier_and_prefix() {
        let query = PatientQuery::new()
            .family(&secret("Schmidt"), StringMatch::Exact)
            .and_then(|query| query.given(&secret("Jo,hn"), StringMatch::StartsWith))
            .and_then(|query| query.birthdate(DatePrefix::Ge, &secret("1923")))
            .map(|query| query.gender(Gender::Other).active(true))
            .expect("a query");
        let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.form().as_bytes())
            .into_owned()
            .collect();
        assert_eq!(
            pairs,
            [
                ("family:exact".to_owned(), "Schmidt".to_owned()),
                ("given".to_owned(), r"Jo\,hn".to_owned()),
                ("birthdate".to_owned(), "ge1923".to_owned()),
                ("gender".to_owned(), "other".to_owned()),
                ("active".to_owned(), "true".to_owned()),
            ],
            "a value's own comma is escaped"
        );
    }

    #[test]
    fn identifiers_and_domains_are_tokens() {
        let query = PatientQuery::new()
            .identifier(Some("urn:oid:2.999.1"), &secret("a|1"))
            .and_then(|query| query.identifier(None, &secret("8675309")))
            .and_then(|query| query.identifier_domains(&["urn:oid:2.999.2", "urn:oid:2.999.3"]))
            .expect("a query");
        let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.form().as_bytes())
            .into_owned()
            .collect();
        assert_eq!(
            pairs,
            [
                ("identifier".to_owned(), r"urn:oid:2.999.1|a\|1".to_owned()),
                ("identifier".to_owned(), "8675309".to_owned()),
                (
                    "identifier".to_owned(),
                    "urn:oid:2.999.2|,urn:oid:2.999.3|".to_owned()
                ),
            ],
            "a domain list is an OR within one parameter"
        );
        assert!(query.names_domain(), "the query names a domain");
        assert!(
            !PatientQuery::new()
                .identifier(None, &secret("8675309"))
                .expect("a query")
                .names_domain(),
            "a bare value names no domain"
        );
    }

    #[test]
    fn invalid_criteria_are_refused_before_anything_is_sent() {
        let empty = PatientQuery::new().family(&secret(""), StringMatch::Exact);
        assert_eq!(empty.err(), Some(InvalidInput::EmptyValue), "empty family");
        let system = PatientQuery::new().identifier(Some("not a uri"), &secret("1"));
        assert_eq!(system.err(), Some(InvalidInput::System), "relative system");
        let domains = PatientQuery::new().identifier_domains(&[]);
        assert_eq!(domains.err(), Some(InvalidInput::EmptyValue), "no domain");
        let id = PatientQuery::new().id(&secret("a/b"));
        assert_eq!(id.err(), Some(InvalidInput::Id), "a slash in an id");
        let date = PatientQuery::new().birthdate(DatePrefix::Eq, &secret("25-07-1923"));
        assert_eq!(date.err(), Some(InvalidInput::Date), "a day-first date");
    }
}
