// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-119 match as the Supplier reads and answers it.
//!
//! `POST [base]/Patient/$match` with the PDQm Match Input `Parameters`, or the
//! bare `Patient` when neither `onlyCertainMatches` nor `count` is used
//! (§2:3.119.4.1.2), answered with the PDQm Match Output Bundle
//! (§2:3.119.4.2.2).

use std::fmt;
use std::num::NonZeroU32;

use fhir_types::codec::{Json, Path, Value};
use fhir_types::r4::bundle::{Bundle, BundleEntry, BundleEntrySearch, BundleLink};
use fhir_types::r4::extension::{Extension, ExtensionValue};
use fhir_types::r4::operation_outcome::OperationOutcome;
use fhir_types::r4::parameters::{Parameters, ParametersParameterValue};
use fhir_types::r4::patient::Patient;
use fhir_types::r4::resource::Resource;
use http::StatusCode;
use url::Url;

use super::Score;
use crate::outcome::IssueType;
use crate::pdqm::matches::MatchGrade;
use crate::server::answer::{Answer, Severity};
use crate::server::format::{BodyError, Format};
use crate::server::token;
use ihe_atna::redact::REDACTED;

/// One `$match` request, the same whichever form carried it.
///
/// `Debug` redacts the Patient; the type has no `Display`.
#[derive(Clone)]
pub struct MatchRequest {
    patient: Box<Patient>,
    only_certain_matches: bool,
    count: Option<NonZeroU32>,
    format: Option<String>,
}

impl MatchRequest {
    /// Reads a `$match` request: the query of its URL (without the `?`) and
    /// its body in `format`.
    ///
    /// The body is a `Parameters` resource held to the PDQm Match Input
    /// Parameters profile (one `resource` Patient, at most one
    /// `onlyCertainMatches` boolean and one `count` integer, no other
    /// parameter), or a bare `Patient`, which stands for the `resource`
    /// parameter alone (§2:3.119.4.1.2). Either Patient is held to the PDQm
    /// Patient Profile for `$match` Input. The query may name `_format` and
    /// `_pretty` (FHIR R4 http.html).
    ///
    /// # Errors
    /// A [`MatchError`] for a request ITI-119 does not admit, among them a
    /// bare Patient with `onlyCertainMatches` or `count` in the query, whose
    /// [`refusal`](MatchError::refusal) is the answer to send.
    pub fn from_request(query: &str, format: Format, body: &[u8]) -> Result<Self, MatchError> {
        let mut asked = None;
        let mut operation_parameter = false;
        for (name, value) in token::url_query(query) {
            match name.as_str() {
                "_format" => {
                    if asked.replace(value).is_some() {
                        return Err(MatchError::SecondParameter);
                    }
                }
                "_pretty" => {}
                "onlyCertainMatches" | "count" => operation_parameter = true,
                _ => return Err(MatchError::UnknownParameter),
            }
        }
        let object = format.decode(body).map_err(MatchError::Body)?;
        let mut request = match object.get("resourceType").and_then(Value::as_str) {
            Some("Patient") => {
                if operation_parameter {
                    return Err(MatchError::BareWithParameters);
                }
                let patient = Patient::from_json(&object, &mut Path::root("Patient"))
                    .map_err(|error| MatchError::Body(BodyError::decode(error)))?;
                Self {
                    patient: Box::new(patient),
                    only_certain_matches: false,
                    count: None,
                    format: None,
                }
            }
            Some("Parameters") => {
                if operation_parameter {
                    return Err(MatchError::ParameterInQuery);
                }
                let parameters = Parameters::from_json(&object, &mut Path::root("Parameters"))
                    .map_err(|error| MatchError::Body(BodyError::decode(error)))?;
                Self::from_parameters(parameters)?
            }
            _ => return Err(MatchError::NotAnInput),
        };
        if request.patient.implicit_rules.is_some()
            || !request.patient.modifier_extension.is_empty()
        {
            return Err(MatchError::Profile);
        }
        request.format = asked;
        Ok(request)
    }

    fn from_parameters(parameters: Parameters) -> Result<Self, MatchError> {
        if parameters.implicit_rules.is_some() {
            return Err(MatchError::Shape { index: None });
        }
        let mut patient = None;
        let mut only_certain_matches = None;
        let mut count = None;
        for (index, parameter) in parameters.parameter.into_iter().enumerate() {
            let shape = MatchError::Shape { index: Some(index) };
            if !parameter.part.is_empty() || !parameter.modifier_extension.is_empty() {
                return Err(shape);
            }
            match (
                parameter.name.value.as_deref(),
                parameter.value,
                parameter.resource,
            ) {
                (Some("resource"), None, Some(Resource::Patient(found))) => {
                    if patient.replace(found).is_some() {
                        return Err(MatchError::SecondParameter);
                    }
                }
                (
                    Some("onlyCertainMatches"),
                    Some(ParametersParameterValue::Boolean(flag)),
                    None,
                ) => {
                    let flag = flag.value.ok_or(shape)?;
                    if only_certain_matches.replace(flag).is_some() {
                        return Err(MatchError::SecondParameter);
                    }
                }
                (Some("count"), Some(ParametersParameterValue::Integer(number)), None) => {
                    let number = number
                        .value
                        .and_then(|number| u32::try_from(number).ok())
                        .and_then(NonZeroU32::new)
                        .ok_or(MatchError::Count)?;
                    if count.replace(number).is_some() {
                        return Err(MatchError::SecondParameter);
                    }
                }
                (Some("resource" | "onlyCertainMatches" | "count"), _, _) => return Err(shape),
                _ => return Err(MatchError::UnknownParameter),
            }
        }
        Ok(Self {
            patient: patient.ok_or(MatchError::NoPatient)?,
            only_certain_matches: only_certain_matches.unwrap_or(false),
            count,
            format: None,
        })
    }

    /// Returns the Patient to match.
    #[must_use]
    pub fn patient(&self) -> &Patient {
        &self.patient
    }

    /// Returns whether only certain matches are asked for
    /// (§2:3.119.4.1.3, Cases 3 to 5).
    #[must_use]
    pub fn only_certain_matches(&self) -> bool {
        self.only_certain_matches
    }

    /// Returns the most matches the answer may hold (§2:3.119.4.1.3,
    /// Case 6).
    #[must_use]
    pub fn count(&self) -> Option<NonZeroU32> {
        self.count
    }

    /// Returns the `_format` the query names, for
    /// [`Format::negotiate`].
    #[must_use]
    pub fn format(&self) -> Option<&str> {
        self.format.as_deref()
    }
}

impl fmt::Debug for MatchRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MatchRequest")
            .field("patient", &REDACTED)
            .field("only_certain_matches", &self.only_certain_matches)
            .field("count", &self.count)
            .field("format", &self.format)
            .finish()
    }
}

/// One Patient the Supplier matched, with its score and grade
/// (§2:3.119.4.2.2.4).
///
/// `Debug` shows the score and the grade and redacts the rest; the type has
/// no `Display`.
#[derive(Clone)]
pub struct MatchEntry {
    full_url: Url,
    patient: Box<Patient>,
    score: Score,
    grade: MatchGrade,
}

impl MatchEntry {
    /// Creates the entry of `patient`, at its absolute URL `full_url` on the
    /// Supplier, matched with `score` and graded `grade`.
    #[must_use]
    pub fn new(full_url: Url, patient: Patient, score: Score, grade: MatchGrade) -> Self {
        Self {
            full_url,
            patient: Box::new(patient),
            score,
            grade,
        }
    }
}

impl fmt::Debug for MatchEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MatchEntry")
            .field("full_url", &REDACTED)
            .field("patient", &REDACTED)
            .field("score", &self.score)
            .field("grade", &self.grade)
            .finish()
    }
}

/// Returns the `200` answer to `request`: the Match Output Bundle
/// (§2:3.119.4.2.2), with the `self` link when one is given.
///
/// The entries are ordered from the highest score to the lowest, ties in the
/// order given (§2:3.119.4.1.3, Case 2). With `onlyCertainMatches`, only the
/// `certain` entries stay, and when more than one does the answer is empty
/// (Cases 3 to 5): each entry is taken for a person of its own, so a Supplier
/// whose one person spans several records answers with one entry for them.
/// `count` then cuts the list (Case 6). Each of `warnings` becomes an
/// `OperationOutcome` entry (Cases 7 and 10).
///
/// # Errors
/// [`MatchSetError::ErrorOutcome`] when a warning holds an issue of `error`
/// or `fatal` severity, which a `200` never carries (§2:3.119.4.1.3,
/// Cases 7 and 10).
pub fn match_set(
    request: &MatchRequest,
    mut entries: Vec<MatchEntry>,
    warnings: Vec<OperationOutcome>,
    self_link: Option<&Url>,
) -> Result<Answer<Bundle>, MatchSetError> {
    if warnings.iter().any(|warning| {
        warning
            .issue
            .iter()
            .any(|issue| matches!(issue.severity.value.as_deref(), Some("error" | "fatal")))
    }) {
        return Err(MatchSetError::ErrorOutcome);
    }
    entries.sort_by(|left, right| right.score.value().total_cmp(&left.score.value()));
    if request.only_certain_matches {
        entries.retain(|entry| entry.grade == MatchGrade::Certain);
        if entries.len() > 1 {
            entries.clear();
        }
    }
    if let Some(count) = request.count {
        entries.truncate(usize::try_from(count.get()).unwrap_or(usize::MAX));
    }
    let mut entry: Vec<BundleEntry> = entries
        .into_iter()
        .map(|entry| BundleEntry {
            full_url: Some(entry.full_url.as_str().into()),
            resource: Some(Resource::Patient(entry.patient)),
            search: Some(BundleEntrySearch {
                extension: vec![Extension {
                    url: MatchGrade::EXTENSION.to_owned(),
                    value: Some(ExtensionValue::Code(entry.grade.code().into())),
                    ..Extension::default()
                }],
                mode: Some("match".into()),
                score: Some(entry.score.decimal().into()),
                ..BundleEntrySearch::default()
            }),
            ..BundleEntry::default()
        })
        .collect();
    entry.extend(warnings.into_iter().map(|warning| BundleEntry {
        resource: Some(Resource::OperationOutcome(Box::new(warning))),
        search: Some(BundleEntrySearch {
            mode: Some("outcome".into()),
            ..BundleEntrySearch::default()
        }),
        ..BundleEntry::default()
    }));
    Ok(Answer::new(
        StatusCode::OK,
        Bundle {
            r#type: "searchset".into(),
            link: self_link
                .map(|url| BundleLink {
                    relation: "self".into(),
                    url: url.as_str().into(),
                    ..BundleLink::default()
                })
                .into_iter()
                .collect(),
            entry,
            ..Bundle::default()
        },
    ))
}

/// Why a request is no `$match` request the Supplier answers.
///
/// No variant carries a value from the request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MatchError {
    /// The body is a bare Patient and the query names `onlyCertainMatches`
    /// or `count`: the bare form stands for the Patient alone
    /// (§2:3.119.4.1.2).
    #[error("a bare Patient is posted with onlyCertainMatches or count")]
    BareWithParameters,
    /// The body is `Parameters` and the query names `onlyCertainMatches` or
    /// `count`, which belong in the body.
    #[error("onlyCertainMatches or count is named in the query")]
    ParameterInQuery,
    /// The request names a parameter `$match` does not define.
    #[error("the request names a parameter $match does not define")]
    UnknownParameter,
    /// A parameter is named twice.
    #[error("a parameter is named more than once")]
    SecondParameter,
    /// The `Parameters` carry no `resource` Patient.
    #[error("the Parameters carry no Patient")]
    NoPatient,
    /// `count` is not a whole number of at least 1.
    #[error("count is not a whole number of at least 1")]
    Count,
    /// A parameter does not hold to the PDQm Match Input Parameters profile;
    /// with no index, the `Parameters` carry `implicitRules`.
    #[error("a parameter does not hold to the PDQm Match Input Parameters profile")]
    Shape {
        /// The parameter's position, when one parameter is at fault.
        index: Option<usize>,
    },
    /// The Patient carries `implicitRules` or a `modifierExtension`, which the
    /// PDQm Patient Profile for `$match` Input forbids.
    #[error("the Patient does not hold to the PDQm Patient Profile for $match Input")]
    Profile,
    /// The body is no FHIR resource.
    #[error("the body is no FHIR R4 resource")]
    Body(#[source] BodyError),
    /// The body is neither `Parameters` nor `Patient`.
    #[error("the body is neither Parameters nor a Patient")]
    NotAnInput,
}

impl MatchError {
    /// Returns the answer to send: a `400` whose issue names the fault
    /// (§2:3.119.4.1.3, Case 9).
    #[must_use]
    pub fn refusal(&self) -> Answer<OperationOutcome> {
        let (code, diagnostics) = match self {
            Self::BareWithParameters => (
                IssueType::Invalid,
                "a bare Patient is posted only without onlyCertainMatches and count",
            ),
            Self::ParameterInQuery => (
                IssueType::Invalid,
                "onlyCertainMatches and count belong in the Parameters",
            ),
            Self::UnknownParameter => (
                IssueType::NotSupported,
                "a parameter $match does not define",
            ),
            Self::SecondParameter => (IssueType::Invalid, "a parameter is named more than once"),
            Self::NoPatient => (IssueType::Required, "the resource parameter is required"),
            Self::Count => (IssueType::Value, "count is a whole number of at least 1"),
            Self::Shape { .. } => (
                IssueType::Structure,
                "the Parameters do not hold to the PDQm Match Input Parameters profile",
            ),
            Self::Profile => (
                IssueType::Invariant,
                "the Patient does not hold to the PDQm Patient Profile for $match Input",
            ),
            Self::Body(_) => (IssueType::Structure, "the body is no FHIR R4 resource"),
            Self::NotAnInput => (
                IssueType::Invalid,
                "the body is neither Parameters nor a Patient",
            ),
        };
        Answer::outcome(StatusCode::BAD_REQUEST, Severity::Error, code, diagnostics)
    }
}

/// Why a Match Output Bundle cannot be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MatchSetError {
    /// A warning holds an issue of `error` or `fatal` severity.
    #[error("a warning outcome holds an error")]
    ErrorOutcome,
}

#[cfg(test)]
mod tests {
    use fhir_types::r4::patient::Patient;
    use url::Url;

    use super::{MatchEntry, MatchError, MatchRequest, match_set};
    use crate::pdqm::matches::MatchGrade;
    use crate::pdqm::server::Score;
    use crate::server::format::Format;

    const BARE: &[u8] = br#"{"resourceType":"Patient","gender":"female"}"#;

    fn entry(id: &str, score: f64, grade: MatchGrade) -> MatchEntry {
        let url = Url::parse(&format!("https://pdq.example.org/fhir/Patient/{id}")).expect("a URL");
        let patient = Patient {
            id: Some(id.to_owned()),
            ..Patient::default()
        };
        MatchEntry::new(url, patient, Score::new(score).expect("a score"), grade)
    }

    #[test]
    fn a_bare_patient_with_an_operation_parameter_is_refused() {
        assert!(
            MatchRequest::from_request("", Format::Json, BARE).is_ok(),
            "bare"
        );
        for query in ["onlyCertainMatches=true", "count=2"] {
            assert_eq!(
                MatchRequest::from_request(query, Format::Json, BARE).err(),
                Some(MatchError::BareWithParameters),
                "{query}"
            );
        }
    }

    #[test]
    fn entries_are_ordered_and_certain_only_is_one_person() {
        let request = MatchRequest::from_request("", Format::Json, BARE).expect("a request");
        let answer = match_set(
            &request,
            vec![
                entry("b", 0.5, MatchGrade::Possible),
                entry("a", 0.9, MatchGrade::Certain),
            ],
            Vec::new(),
            None,
        )
        .expect("a Bundle");
        let ids: Vec<Option<&str>> = answer
            .resource()
            .entry
            .iter()
            .map(|entry| match &entry.resource {
                Some(fhir_types::r4::resource::Resource::Patient(patient)) => patient.id.as_deref(),
                _ => None,
            })
            .collect();
        assert_eq!(ids, [Some("a"), Some("b")], "most likely first");
        let certain = br#"{"resourceType":"Parameters","parameter":[{"name":"resource","resource":{"resourceType":"Patient"}},{"name":"onlyCertainMatches","valueBoolean":true}]}"#;
        let request = MatchRequest::from_request("", Format::Json, certain).expect("a request");
        let two = match_set(
            &request,
            vec![
                entry("a", 0.99, MatchGrade::Certain),
                entry("c", 0.98, MatchGrade::Certain),
            ],
            Vec::new(),
            None,
        )
        .expect("a Bundle");
        assert!(two.resource().entry.is_empty(), "§2:3.119.4.1.3 Case 5");
    }
}
