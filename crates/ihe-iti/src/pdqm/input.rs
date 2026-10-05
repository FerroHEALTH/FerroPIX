// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The input of an ITI-119 Patient Demographics Match (§2:3.119.4.1.2).
//!
//! A Patient holding the demographics to match is posted in a `Parameters`
//! resource with the `onlyCertainMatches` and `count` parameters, as the PDQm
//! Match Input Parameters profile defines it.

use std::fmt;
use std::num::NonZeroU16;

use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::parameters::{Parameters, ParametersParameter, ParametersParameterValue};
use fhir_types::r4::patient::Patient;
use fhir_types::r4::resource::Resource;
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::error::InvalidInput;

/// The demographics one ITI-119 request asks the Supplier to match.
///
/// Every value is directly identifying and travels only in the request body
/// to the Supplier. `Debug` counts the identifiers and never shows one, and
/// the type has no `Display`.
///
/// # Examples
///
/// ```
/// use ihe_iti::pdqm::input::MatchInput;
/// use secrecy::SecretString;
///
/// let input = MatchInput::new("urn:oid:2.999.1", &SecretString::from("12345"))?
///     .only_certain_matches(true);
/// assert!(!format!("{input:?}").contains("12345"));
/// # Ok::<(), ihe_iti::pdqm::error::InvalidInput>(())
/// ```
pub struct MatchInput {
    identifiers: Vec<(String, SecretString)>,
    only_certain_matches: Option<bool>,
    count: Option<NonZeroU16>,
}

impl MatchInput {
    /// Creates an input whose Patient carries `value` in the identifier
    /// system `system`; an input names at least one demographic.
    ///
    /// # Errors
    /// [`InvalidInput::System`] when `system` is not an absolute URI, and
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn new(system: &str, value: &SecretString) -> Result<Self, InvalidInput> {
        Self {
            identifiers: Vec::new(),
            only_certain_matches: None,
            count: None,
        }
        .identifier(system, value)
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
    /// Consumer audit profile, `entity:patient`).
    #[cfg(feature = "balp")]
    pub(super) fn sole_identifier(&self) -> Option<(&str, &SecretString)> {
        match self.identifiers.as_slice() {
            [(system, value)] => Some((system.as_str(), value)),
            _ => None,
        }
    }

    /// The `Parameters` resource the request posts: the input Patient in the
    /// `resource` parameter, then `onlyCertainMatches` and `count` when set
    /// (the PDQm Match Input Parameters profile).
    ///
    /// The resource holds every value, so it is handed to the HTTP client and
    /// never kept, logged or put into an error.
    pub(super) fn parameters(&self) -> Parameters {
        let patient = Patient {
            identifier: self
                .identifiers
                .iter()
                .map(|(system, value)| Identifier {
                    system: Some(system.as_str().into()),
                    value: Some(value.expose_secret().into()),
                    ..Identifier::default()
                })
                .collect(),
            ..Patient::default()
        };
        let mut parameter = vec![ParametersParameter {
            name: "resource".into(),
            resource: Some(Resource::Patient(Box::new(patient))),
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

impl fmt::Debug for MatchInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MatchInput")
            .field("identifiers", &self.identifiers.len())
            .field("only_certain_matches", &self.only_certain_matches)
            .field("count", &self.count)
            .finish()
    }
}
