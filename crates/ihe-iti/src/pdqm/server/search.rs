// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-78 search as the Supplier reads it.
//!
//! `GET [base]/Patient?<parameters>` or `POST [base]/Patient/_search` with
//! the same parameters in a form body, which the Supplier "SHALL support
//! both" (§2:3.78.4.1.2): the parameters of §2:3.78.4.1.2.1, the `:exact`
//! modifier on each string parameter (§2:3.78.4.1.2.2), repeated parameters
//! as AND and a `,` between values as OR (FHIR R4 search, "Combining",
//! <http://hl7.org/fhir/R4/search.html#combining>), and the domain filter
//! (§2:3.78.4.1.2.3).

use std::fmt;

use fhir_types::r4::operation_outcome::OperationOutcome;
use http::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::outcome::IssueType;
use crate::pdqm::query::{DatePrefix, Gender, StringMatch, fhir_date};
use crate::server::answer::{Answer, Severity};
use crate::server::token;
use ihe_atna::redact::REDACTED;

/// The system of the `administrative-gender` code system, which a `gender`
/// token may name.
const GENDER_SYSTEM: &str = "http://hl7.org/fhir/administrative-gender";

/// A string search parameter of ITI-78 (§2:3.78.4.1.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StringParameter {
    /// `family`
    Family,
    /// `given`
    Given,
    /// `address`: any part of the address.
    Address,
    /// `address-city`
    AddressCity,
    /// `address-country`
    AddressCountry,
    /// `address-postalcode`
    AddressPostalcode,
    /// `address-state`
    AddressState,
    /// `mothersMaidenName`, on the `patient-mothersMaidenName` extension.
    MothersMaidenName,
}

impl StringParameter {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "family" => Self::Family,
            "given" => Self::Given,
            "address" => Self::Address,
            "address-city" => Self::AddressCity,
            "address-country" => Self::AddressCountry,
            "address-postalcode" => Self::AddressPostalcode,
            "address-state" => Self::AddressState,
            "mothersMaidenName" => Self::MothersMaidenName,
            _ => return None,
        })
    }
}

/// A token value: an identifier or a contact value, with or without its
/// system (FHIR R4 search, token).
///
/// `Debug` shows the system and redacts the value; the type has no `Display`.
#[derive(Clone)]
pub struct Token {
    system: Option<String>,
    value: SecretString,
}

impl Token {
    /// Returns the system: `None` for a value in any system, the empty string
    /// for a value with no system (`|value`).
    #[must_use]
    pub fn system(&self) -> Option<&str> {
        self.system.as_deref()
    }

    /// Returns the value.
    #[must_use]
    pub fn value(&self) -> &SecretString {
        &self.value
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Token")
            .field("system", &self.system)
            .field("value", &REDACTED)
            .finish()
    }
}

/// A `birthdate` value: a FHIR `date` with its comparison prefix (FHIR R4
/// search, date).
#[derive(Clone)]
pub struct DateValue {
    prefix: DatePrefix,
    date: SecretString,
}

impl DateValue {
    /// Returns the comparison.
    #[must_use]
    pub fn prefix(&self) -> DatePrefix {
        self.prefix
    }

    /// Returns the date: `YYYY`, `YYYY-MM` or `YYYY-MM-DD`.
    #[must_use]
    pub fn date(&self) -> &SecretString {
        &self.date
    }
}

impl fmt::Debug for DateValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DateValue")
            .field("prefix", &self.prefix)
            .field("date", &REDACTED)
            .finish()
    }
}

/// One search parameter as read: its values are alternatives (OR), and the
/// criteria of one search all hold (AND).
#[derive(Clone)]
#[non_exhaustive]
pub enum Criterion {
    /// `_id`: the Patient's logical id, an exact match (§2:3.78.4.1.2.1).
    Id(Vec<SecretString>),
    /// `active`.
    Active(Vec<bool>),
    /// A string parameter, matched as `matching` says.
    Text {
        /// The parameter.
        parameter: StringParameter,
        /// The default prefix match, or `:exact`.
        matching: StringMatch,
        /// The values, unescaped.
        values: Vec<SecretString>,
    },
    /// `identifier`, each value an identifier (the domain filter is
    /// [`PatientSearch::domains`]).
    Identifier(Vec<Token>),
    /// `telecom`.
    Telecom(Vec<Token>),
    /// `birthdate`.
    Birthdate(Vec<DateValue>),
    /// `gender`: one code, the only value the token may hold
    /// (§2:3.78.4.1.2.1).
    Gender(Gender),
}

impl fmt::Debug for Criterion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id(values) => write!(f, "Id({} values)", values.len()),
            Self::Active(values) => f.debug_tuple("Active").field(values).finish(),
            Self::Text {
                parameter,
                matching,
                values,
            } => f
                .debug_struct("Text")
                .field("parameter", parameter)
                .field("matching", matching)
                .field("values", &values.len())
                .finish(),
            Self::Identifier(values) => f.debug_tuple("Identifier").field(values).finish(),
            Self::Telecom(values) => f.debug_tuple("Telecom").field(values).finish(),
            Self::Birthdate(values) => f.debug_tuple("Birthdate").field(values).finish(),
            Self::Gender(gender) => f.debug_tuple("Gender").field(gender).finish(),
        }
    }
}

/// One ITI-78 search, as the Supplier is to answer it.
///
/// `Debug` shows the criteria with their values redacted and counts the
/// dropped parameters; the type has no `Display`.
#[derive(Clone)]
pub struct PatientSearch {
    criteria: Vec<Criterion>,
    domains: Vec<String>,
    count: Option<u32>,
    format: Option<String>,
    respected: Vec<(String, SecretString)>,
    dropped: Vec<String>,
}

impl PatientSearch {
    /// Reads the query string of a `GET` (without its `?`), percent-encoded
    /// as a URL query is (RFC 3986), so a `+` is itself.
    ///
    /// Every parameter of §2:3.78.4.1.2.1 is read, `_count` (FHIR R4 search,
    /// paging) and `_format` beside them, and `_pretty` admitted. A parameter
    /// the transaction does not name, and one with a modifier other than
    /// `:exact` on a string parameter, is dropped and reported by
    /// [`PatientSearch::dropped`]: the Supplier "SHOULD" ignore the first
    /// (§2:3.78.4.1.2.1) and "MAY" ignore the second (§2:3.78.4.1.2.2), and
    /// the `self` link says what was respected (§2:3.78.4.1.2.1.1).
    ///
    /// # Errors
    /// A [`SearchError`] for a value the search cannot be run with, whose
    /// [`refusal`](SearchError::refusal) is the answer to send.
    pub fn from_query(query: &str) -> Result<Self, SearchError> {
        Self::from_pairs(token::url_query(query))
    }

    /// Reads the body of a `POST [base]/Patient/_search`, an
    /// `application/x-www-form-urlencoded` form in which a `+` is a space
    /// (FHIR R4 http.html, search), as [`PatientSearch::from_query`] reads a
    /// query.
    ///
    /// # Errors
    /// As [`PatientSearch::from_query`].
    pub fn from_form(body: &[u8]) -> Result<Self, SearchError> {
        Self::from_pairs(token::form(body))
    }

    fn from_pairs(pairs: Vec<(String, String)>) -> Result<Self, SearchError> {
        let mut search = Self {
            criteria: Vec::new(),
            domains: Vec::new(),
            count: None,
            format: None,
            respected: Vec::new(),
            dropped: Vec::new(),
        };
        for (name, value) in pairs {
            search.read(&name, SecretString::from(value))?;
        }
        Ok(search)
    }

    fn read(&mut self, name: &str, value: SecretString) -> Result<(), SearchError> {
        let (base, modifier) = match name.split_once(':') {
            Some((base, modifier)) => (base, Some(modifier)),
            None => (name, None),
        };
        let text = value.expose_secret();
        let criterion = match (base, modifier) {
            ("_pretty", None) => return Ok(()),
            ("_format", None) => {
                if self.format.replace(text.to_owned()).is_some() {
                    return Err(SearchError::SecondFormat);
                }
                return Ok(());
            }
            ("_count", None) => {
                let count = text.parse().map_err(|_not_a_count| SearchError::Count)?;
                if self.count.replace(count).is_some() {
                    return Err(SearchError::SecondCount);
                }
                self.respected.push((name.to_owned(), value));
                return Ok(());
            }
            ("_id", None | Some("exact")) => Criterion::Id(alternatives(text, id)?),
            ("active", None) => Criterion::Active(alternatives(text, active)?),
            ("identifier", None) => {
                let tokens = alternatives(text, |part| Ok(token::token(part)))?;
                if tokens.iter().all(|token| {
                    token.system.as_deref().is_some_and(|s| !s.is_empty())
                        && token.code.expose_secret().is_empty()
                }) {
                    self.domains
                        .extend(tokens.into_iter().filter_map(|token| token.system));
                    self.respected.push((name.to_owned(), value));
                    return Ok(());
                }
                Criterion::Identifier(identifiers(tokens)?)
            }
            ("telecom", None) => {
                let tokens = alternatives(text, |part| Ok(token::token(part)))?;
                Criterion::Telecom(identifiers(tokens)?)
            }
            ("birthdate", None) => Criterion::Birthdate(alternatives(text, birthdate)?),
            ("gender", None) => Criterion::Gender(gender(text)?),
            (base, modifier) => {
                let Some(parameter) = StringParameter::named(base)
                    .filter(|_| matches!(modifier, None | Some("exact")))
                else {
                    self.dropped.push(name.to_owned());
                    return Ok(());
                };
                Criterion::Text {
                    parameter,
                    matching: if modifier.is_some() {
                        StringMatch::Exact
                    } else {
                        StringMatch::StartsWith
                    },
                    values: alternatives(text, |part| {
                        Ok(SecretString::from(token::unescape(part)))
                    })?,
                }
            }
        };
        self.criteria.push(criterion);
        self.respected.push((name.to_owned(), value));
        Ok(())
    }

    /// Returns the criteria, in the request's order, each to hold.
    #[must_use]
    pub fn criteria(&self) -> &[Criterion] {
        &self.criteria
    }

    /// Returns the identifier domains the matched Patients' identifiers are
    /// pruned to, from every `identifier=system|` value (§2:3.78.4.1.2.3):
    /// none keeps every domain (§2:3.78.4.1.3, Cases 1 and 2).
    #[must_use]
    pub fn domains(&self) -> &[String] {
        &self.domains
    }

    /// Returns the page size `_count` asks for.
    #[must_use]
    pub fn count(&self) -> Option<u32> {
        self.count
    }

    /// Returns the `_format` the request names, for
    /// [`Format::negotiate`](crate::server::format::Format::negotiate).
    #[must_use]
    pub fn format(&self) -> Option<&str> {
        self.format.as_deref()
    }

    /// Returns the parameters dropped, each as the request named it, with
    /// its modifier.
    #[must_use]
    pub fn dropped(&self) -> &[String] {
        &self.dropped
    }

    /// Returns the `self` link of the answer: `endpoint`, the Supplier's
    /// `[base]/Patient`, with every parameter the search respected, so a
    /// dropped one is absent (§2:3.78.4.1.2.1.1; FHIR R4 search, "Handling
    /// Errors").
    #[must_use]
    pub fn self_link(&self, endpoint: &Url) -> SelfLink {
        let mut url = endpoint.clone();
        // NOTE: RFC 3986 §2.1, a `+` in a URL is itself (as `from_query` reads it),
        // so a space is written `%20` and never the form encoding's `+`.
        let query: Vec<String> = self
            .respected
            .iter()
            .map(|(name, value)| format!("{}={}", url_part(name), url_part(value.expose_secret())))
            .collect();
        url.set_query((!query.is_empty()).then(|| query.join("&")).as_deref());
        SelfLink(url)
    }
}

impl fmt::Debug for PatientSearch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PatientSearch")
            .field("criteria", &self.criteria)
            .field("domains", &self.domains)
            .field("count", &self.count)
            .field("format", &self.format)
            .field("dropped", &self.dropped.len())
            .finish_non_exhaustive()
    }
}

/// The `self` link of an answer, which carries the search's values.
///
/// `Debug` redacts it and the type has no `Display`.
#[derive(Clone)]
pub struct SelfLink(Url);

impl SelfLink {
    /// Returns the link.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.0
    }
}

impl fmt::Debug for SelfLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SelfLink({REDACTED})")
    }
}

/// `text` percent-encoded for a URL query, every byte outside the form
/// encoding's unreserved set escaped and a space as `%20`.
fn url_part(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

/// The alternatives of one value, each read by `read`; an empty one is
/// refused.
fn alternatives<T>(
    text: &str,
    read: impl Fn(&str) -> Result<T, SearchError>,
) -> Result<Vec<T>, SearchError> {
    token::split(text, ',')
        .into_iter()
        .map(|part| {
            if part.is_empty() {
                Err(SearchError::EmptyValue)
            } else {
                read(part)
            }
        })
        .collect()
}

fn id(part: &str) -> Result<SecretString, SearchError> {
    let id = token::unescape(part);
    let valid = (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'));
    if valid {
        Ok(SecretString::from(id))
    } else {
        Err(SearchError::Id)
    }
}

fn active(part: &str) -> Result<bool, SearchError> {
    match part {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(SearchError::Active),
    }
}

/// The tokens of an `identifier` or `telecom` criterion, each with a value.
fn identifiers(tokens: Vec<token::Token>) -> Result<Vec<Token>, SearchError> {
    tokens
        .into_iter()
        .map(|token| {
            if token.code.expose_secret().is_empty() {
                Err(SearchError::DomainMixed)
            } else {
                Ok(Token {
                    system: token.system,
                    value: token.code,
                })
            }
        })
        .collect()
}

fn birthdate(part: &str) -> Result<DateValue, SearchError> {
    let (prefix, date) = DatePrefix::split(part);
    if fhir_date(date) {
        Ok(DateValue {
            prefix,
            date: SecretString::from(date),
        })
    } else {
        Err(SearchError::Date)
    }
}

/// The one gender code of a `gender` value (§2:3.78.4.1.2.1).
fn gender(text: &str) -> Result<Gender, SearchError> {
    let parts = token::split(text, ',');
    let [part] = parts.as_slice() else {
        return Err(SearchError::GenderValues);
    };
    let token = token::token(part);
    if token
        .system
        .as_deref()
        .is_some_and(|system| system != GENDER_SYSTEM)
    {
        return Err(SearchError::GenderCode);
    }
    Gender::from_code(token.code.expose_secret()).ok_or(SearchError::GenderCode)
}

/// Why a search cannot be run (FHIR R4 search, "Handling Errors": a search
/// that cannot be executed answers a `4xx` with an `OperationOutcome`).
///
/// No variant carries a value from the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SearchError {
    /// A value, or one of its alternatives, is empty.
    #[error("a search value is empty")]
    EmptyValue,
    /// An `_id` value is not a FHIR `id`.
    #[error("an _id value is not a FHIR id")]
    Id,
    /// An `active` value is neither `true` nor `false`.
    #[error("an active value is not a boolean")]
    Active,
    /// A `birthdate` value is not a FHIR `date` behind an optional prefix.
    #[error("a birthdate value is not a date")]
    Date,
    /// A `gender` token holds more than one value, where a single code
    /// "SHALL be specified as the only value of the token"
    /// (§2:3.78.4.1.2.1).
    #[error("a gender token holds more than one value")]
    GenderValues,
    /// A `gender` code is not in `administrative-gender`, a required
    /// binding.
    #[error("a gender code is not an administrative gender")]
    GenderCode,
    /// An `identifier` parameter mixes domain-filter values (`system|`) with
    /// identifiers, or a `telecom` value has no value.
    #[error("an identifier parameter mixes domains and identifiers")]
    DomainMixed,
    /// `_count` is not a non-negative whole number.
    #[error("_count is not a non-negative whole number")]
    Count,
    /// `_count` is named twice.
    #[error("_count is named more than once")]
    SecondCount,
    /// `_format` is named twice.
    #[error("_format is named more than once")]
    SecondFormat,
}

impl SearchError {
    /// Returns the answer to send: a `400` whose issue names the fault.
    #[must_use]
    pub fn refusal(self) -> Answer<OperationOutcome> {
        let (code, diagnostics) = match self {
            Self::EmptyValue => (IssueType::Invalid, "a search value is empty"),
            Self::Id => (IssueType::Value, "an _id value is not a FHIR id"),
            Self::Active => (IssueType::Value, "an active value is not true or false"),
            Self::Date => (IssueType::Value, "a birthdate value is not a FHIR date"),
            Self::GenderValues => (
                IssueType::Invalid,
                "a single administrative gender code is the only value of gender",
            ),
            Self::GenderCode => (
                IssueType::CodeInvalid,
                "a gender code is not an administrative gender",
            ),
            Self::DomainMixed => (
                IssueType::Invalid,
                "an identifier parameter mixes domains and identifiers",
            ),
            Self::Count => (IssueType::Value, "_count is not a whole number"),
            Self::SecondCount => (IssueType::Invalid, "_count is allowed once"),
            Self::SecondFormat => (IssueType::Invalid, "_format is allowed once"),
        };
        Answer::outcome(StatusCode::BAD_REQUEST, Severity::Error, code, diagnostics)
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;
    use url::Url;

    use super::{Criterion, PatientSearch, SearchError, StringParameter};
    use crate::pdqm::query::{DatePrefix, Gender, StringMatch};

    #[test]
    fn and_and_or_are_read_as_criteria() {
        let search = PatientSearch::from_query(
            "family=MOHR&given=ALICE,ALISSA&given:exact=Al%5C,ice&birthdate=ge1958&gender=female",
        )
        .expect("a search");
        let criteria = search.criteria();
        assert_eq!(criteria.len(), 5, "one criterion per parameter");
        let Criterion::Text {
            parameter: StringParameter::Given,
            matching: StringMatch::StartsWith,
            values,
        } = &criteria[1]
        else {
            panic!("given, two alternatives");
        };
        assert_eq!(values.len(), 2, "OR within one parameter");
        let Criterion::Text {
            matching: StringMatch::Exact,
            values,
            ..
        } = &criteria[2]
        else {
            panic!("given:exact");
        };
        assert_eq!(values[0].expose_secret(), "Al,ice", "the escaped comma");
        let Criterion::Birthdate(dates) = &criteria[3] else {
            panic!("birthdate");
        };
        assert_eq!(dates[0].prefix(), DatePrefix::Ge);
        assert!(matches!(criteria[4], Criterion::Gender(Gender::Female)));
    }

    #[test]
    fn the_get_query_and_the_post_form_read_alike() {
        let query = PatientSearch::from_query("family=van%20Dam&telecom=%2B31201234567")
            .expect("a GET search");
        let form = PatientSearch::from_form(b"family=van+Dam&telecom=%2B31201234567")
            .expect("a POST search");
        for search in [&query, &form] {
            let Criterion::Text { values, .. } = &search.criteria()[0] else {
                panic!("family");
            };
            assert_eq!(values[0].expose_secret(), "van Dam", "a space either way");
            let Criterion::Telecom(tokens) = &search.criteria()[1] else {
                panic!("telecom");
            };
            assert_eq!(tokens[0].value().expose_secret(), "+31201234567");
        }
        let plus = PatientSearch::from_query("telecom=+31201234567").expect("a GET search");
        let Criterion::Telecom(tokens) = &plus.criteria()[0] else {
            panic!("telecom");
        };
        assert_eq!(
            tokens[0].value().expose_secret(),
            "+31201234567",
            "a + in a URL is itself (RFC 3986)"
        );
    }

    #[test]
    fn a_gender_with_two_values_is_refused() {
        assert_eq!(
            PatientSearch::from_query("gender=male,female").err(),
            Some(SearchError::GenderValues)
        );
        assert!(
            PatientSearch::from_query("gender=http://hl7.org/fhir/administrative-gender%7Cmale")
                .is_ok(),
            "the code system may be named"
        );
    }

    #[test]
    fn the_domain_filter_is_kept_apart_and_in_the_self_link() {
        let search = PatientSearch::from_query(
            "family=SMITH&identifier=urn:oid:1.2.3%7C,urn:oid:4.5.6%7C&family:contains=SM",
        )
        .expect("a search");
        assert_eq!(search.domains(), ["urn:oid:1.2.3", "urn:oid:4.5.6"]);
        assert_eq!(
            search.dropped(),
            ["family:contains"],
            "the modifier is ignored"
        );
        let endpoint = Url::parse("https://pdq.example.org/fhir/Patient").expect("a URL");
        assert_eq!(
            search.self_link(&endpoint).url().as_str(),
            "https://pdq.example.org/fhir/Patient?family=SMITH&identifier=urn%3Aoid%3A1.2.3%7C%2Curn%3Aoid%3A4.5.6%7C",
            "the dropped parameter is absent"
        );
        assert_eq!(
            format!("{:?}", search.self_link(&endpoint)),
            "SelfLink(***)"
        );
    }

    #[test]
    fn the_self_link_reads_back_as_the_same_search() {
        let form =
            PatientSearch::from_form(b"family=van+Dam&telecom=%2B31201234567&address-state=t%3D1+")
                .expect("a POST search");
        let endpoint = Url::parse("https://pdq.example.org/fhir/Patient").expect("a URL");
        let link = form.self_link(&endpoint);
        assert_eq!(
            link.url().query(),
            Some("family=van%20Dam&telecom=%2B31201234567&address-state=t%3D1%20"),
            "a space is %20 and a + is %2B"
        );
        let again = PatientSearch::from_query(link.url().query().unwrap_or_default())
            .expect("the self link reads");
        let values = |search: &PatientSearch| -> Vec<String> {
            search
                .criteria()
                .iter()
                .map(|criterion| match criterion {
                    Criterion::Text { values, .. } => values[0].expose_secret().to_owned(),
                    Criterion::Telecom(tokens) => tokens[0].value().expose_secret().to_owned(),
                    _ => panic!("a criterion this test does not send"),
                })
                .collect()
        };
        assert_eq!(values(&again), ["van Dam", "+31201234567", "t=1 "]);
        assert_eq!(values(&again), values(&form), "the same values");
    }
}
