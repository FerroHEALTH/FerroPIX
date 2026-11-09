// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The demographic criteria of an ITI-78 search: the Patient search
//! parameters a Supplier processes (§2:3.78.4.1.2.1) and the `:exact`
//! modifier it supports on each string parameter (§2:3.78.4.1.2.2).

use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use url::Url;

use fhir_search::date::{Precision, SearchDate};
use fhir_search::escape::escape;
use fhir_search::prefix::Prefix;

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
    /// The gender `code` names in `administrative-gender`, or `None`.
    #[cfg(feature = "pdqm-server")]
    pub(super) fn from_code(code: &str) -> Option<Self> {
        match code {
            "male" => Some(Self::Male),
            "female" => Some(Self::Female),
            "other" => Some(Self::Other),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }

    pub(super) fn code(self) -> &'static str {
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
/// type has no `Display`. The default query has no criteria.
///
/// # Examples
///
/// ```
/// use fhir_search::prefix::Prefix;
/// use ihe_iti::pdqm::query::{Gender, PatientQuery, StringMatch};
/// use secrecy::SecretString;
///
/// let query = PatientQuery::default()
///     .family(&SecretString::from("Schmidt"), StringMatch::Exact)?
///     .birthdate(Prefix::Eq, &SecretString::from("1923-07-25"))?
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
    /// (<http://hl7.org/fhir/R4/datatypes.html#date>): a year, a month or a
    /// day of the calendar, read by the FHIR search grammar.
    pub fn birthdate(self, prefix: Prefix, date: &SecretString) -> Result<Self, InvalidInput> {
        let read =
            SearchDate::parse(date.expose_secret()).map_err(|_not_a_date| InvalidInput::Date)?;
        if read.precision() > Precision::Day {
            return Err(InvalidInput::Date);
        }
        // NOTE: FHIR R4 search.html §3.1.1.4.5, a value with no prefix is `eq`, so
        // the query writes none for it.
        let code = if prefix == Prefix::Eq {
            ""
        } else {
            prefix.code()
        };
        let value = format!("{code}{read}");
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

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use fhir_search::prefix::Prefix;

    use super::{Gender, InvalidInput, PatientQuery, StringMatch};

    fn secret(text: &str) -> SecretString {
        SecretString::from(text)
    }

    #[test]
    fn a_fhir_date_has_a_year_and_optionally_a_month_and_a_day() {
        let fhir_date = |date: &str| {
            PatientQuery::default()
                .birthdate(Prefix::Eq, &secret(date))
                .is_ok()
        };
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
            "1923-02-30",
            "ge1923",
        ] {
            assert!(!fhir_date(date), "{date} is not a FHIR date");
        }
    }

    #[test]
    fn the_form_carries_each_parameter_with_its_modifier_and_prefix() {
        let query = PatientQuery::default()
            .family(&secret("Schmidt"), StringMatch::Exact)
            .and_then(|query| query.given(&secret("Jo,hn"), StringMatch::StartsWith))
            .and_then(|query| query.birthdate(Prefix::Ge, &secret("1923")))
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
        let query = PatientQuery::default()
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
            !PatientQuery::default()
                .identifier(None, &secret("8675309"))
                .expect("a query")
                .names_domain(),
            "a bare value names no domain"
        );
    }

    #[test]
    fn invalid_criteria_are_refused_before_anything_is_sent() {
        let empty = PatientQuery::default().family(&secret(""), StringMatch::Exact);
        assert_eq!(empty.err(), Some(InvalidInput::EmptyValue), "empty family");
        let system = PatientQuery::default().identifier(Some("not a uri"), &secret("1"));
        assert_eq!(system.err(), Some(InvalidInput::System), "relative system");
        let domains = PatientQuery::default().identifier_domains(&[]);
        assert_eq!(domains.err(), Some(InvalidInput::EmptyValue), "no domain");
        let id = PatientQuery::default().id(&secret("a/b"));
        assert_eq!(id.err(), Some(InvalidInput::Id), "a slash in an id");
        let date = PatientQuery::default().birthdate(Prefix::Eq, &secret("25-07-1923"));
        assert_eq!(date.err(), Some(InvalidInput::Date), "a day-first date");
    }
}
