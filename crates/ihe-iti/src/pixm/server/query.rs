// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-83 request as the Manager reads it.
//!
//! `GET [base]/Patient/$ihe-pix?sourceIdentifier=…{&targetSystem=…}{&_format=…}`
//! (§2:3.83.4.1.2), or the same input parameters posted to
//! `[base]/Patient/$ihe-pix` in a `Parameters` body (the Query Parameters In
//! profile; FHIR R4 Operations,
//! <http://hl7.org/fhir/R4/operations.html#executing>).

use fhir_types::codec::{Json, Path, Value};
use fhir_types::r4::operation_outcome::OperationOutcome;
use fhir_types::r4::parameters::{Parameters, ParametersParameter, ParametersParameterValue};
use http::StatusCode;
use secrecy::{ExposeSecret, SecretString};

use super::Refusal;
use crate::outcome::IssueType;
use crate::pixm::error::InvalidInput;
use crate::pixm::identifier::{SourceIdentifier, TargetSystem};
use crate::server::answer::{Answer, Severity};
use crate::server::format::{BodyError, Format};
use crate::server::token;

/// One `$ihe-pix` request: the patient asked about, the domains asked for,
/// and the `_format` asked, the same whichever form carried it.
///
/// `Debug` shows the domains and redacts the identifier value; the type has
/// no `Display`.
#[derive(Debug, Clone)]
pub struct PixQuery {
    source: SourceIdentifier,
    targets: Vec<TargetSystem>,
    format: Option<String>,
}

impl PixQuery {
    /// Reads the query string of a `GET` (without its `?`), percent-encoded
    /// as a URL query is.
    ///
    /// `_format` and `_pretty`, which FHIR R4 gives every interaction (FHIR
    /// R4 http.html, <http://hl7.org/fhir/R4/http.html#parameters>), are
    /// read beside the operation's own two parameters.
    ///
    /// # Errors
    /// A [`QueryError`] for a request ITI-83 does not admit, whose
    /// [`refusal`](QueryError::refusal) is the answer to send.
    pub fn from_query(query: &str) -> Result<Self, QueryError> {
        let mut reader = Reader::default();
        for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
            let value = SecretString::from(value.into_owned());
            match name.as_ref() {
                "sourceIdentifier" => reader.source(&value)?,
                "targetSystem" => reader.target(value.expose_secret())?,
                "_format" => reader.format(value.expose_secret())?,
                "_pretty" => {}
                _ => return Err(QueryError::UnknownParameter),
            }
        }
        reader.finish()
    }

    /// Reads the body of a `POST` in `format`: a `Parameters` resource whose
    /// parameters are `sourceIdentifier`, `targetSystem` and `_format`, each a
    /// `valueString` (the Query Parameters In profile).
    ///
    /// # Errors
    /// As [`PixQuery::from_parameters`], and [`QueryError::Body`] or
    /// [`QueryError::NotParameters`] for a body that is no `Parameters`
    /// resource.
    pub fn from_body(format: Format, body: &[u8]) -> Result<Self, QueryError> {
        let object = format.decode(body).map_err(QueryError::Body)?;
        if object.get("resourceType").and_then(Value::as_str) != Some("Parameters") {
            return Err(QueryError::NotParameters);
        }
        let parameters = Parameters::from_json(&object, &mut Path::root("Parameters"))
            .map_err(|error| QueryError::Body(BodyError::decode(error)))?;
        Self::from_parameters(&parameters)
    }

    /// Reads a `Parameters` resource already decoded.
    ///
    /// # Errors
    /// A [`QueryError`] for a request ITI-83 does not admit, whose
    /// [`refusal`](QueryError::refusal) is the answer to send.
    pub fn from_parameters(parameters: &Parameters) -> Result<Self, QueryError> {
        if parameters.implicit_rules.is_some() {
            return Err(QueryError::Shape { index: None });
        }
        let mut reader = Reader::default();
        for (index, parameter) in parameters.parameter.iter().enumerate() {
            let value = string(index, parameter)?;
            match parameter.name.value.as_deref() {
                Some("sourceIdentifier") => reader.source(&value)?,
                Some("targetSystem") => reader.target(value.expose_secret())?,
                Some("_format") => reader.format(value.expose_secret())?,
                _ => return Err(QueryError::UnknownParameter),
            }
        }
        reader.finish()
    }

    /// Returns the patient asked about: the `sourceIdentifier`
    /// (§2:3.83.4.1.2.1).
    #[must_use]
    pub fn source(&self) -> &SourceIdentifier {
        &self.source
    }

    /// Returns the domains asked for, in the request's order: every
    /// `targetSystem` (§2:3.83.4.1.2.2). None asks for every domain.
    #[must_use]
    pub fn targets(&self) -> &[TargetSystem] {
        &self.targets
    }

    /// Returns the `_format` the request names, for
    /// [`Format::negotiate`].
    #[must_use]
    pub fn format(&self) -> Option<&str> {
        self.format.as_deref()
    }
}

/// The value of a posted parameter: a `valueString` and nothing else (the
/// Query Parameters In profile).
fn string(index: usize, parameter: &ParametersParameter) -> Result<SecretString, QueryError> {
    if parameter.resource.is_some()
        || !parameter.part.is_empty()
        || !parameter.modifier_extension.is_empty()
    {
        return Err(QueryError::Shape { index: Some(index) });
    }
    match &parameter.value {
        Some(ParametersParameterValue::String(text)) => text
            .value
            .as_deref()
            .map(SecretString::from)
            .ok_or(QueryError::Shape { index: Some(index) }),
        _ => Err(QueryError::Shape { index: Some(index) }),
    }
}

/// The parameters read so far.
#[derive(Default)]
struct Reader {
    source: Option<SourceIdentifier>,
    targets: Vec<TargetSystem>,
    format: Option<String>,
}

impl Reader {
    fn source(&mut self, value: &SecretString) -> Result<(), QueryError> {
        if self.source.is_some() {
            return Err(QueryError::SecondSourceIdentifier);
        }
        let token = token::token(value.expose_secret());
        let system = token
            .system
            .filter(|system| !system.is_empty())
            .ok_or(QueryError::NoAssigningAuthority)?;
        // NOTE: PIXm §2:3.83.4.2.2.3, a system that is no absolute URI names no
        // domain the Manager can recognise, so it gets that refusal.
        let source = SourceIdentifier::new(system, token.code).map_err(|error| match error {
            InvalidInput::EmptyValue => QueryError::EmptyValue,
            _ => QueryError::SourceDomain,
        })?;
        self.source = Some(source);
        Ok(())
    }

    fn target(&mut self, value: &str) -> Result<(), QueryError> {
        let system = token::unescape(value);
        if system.is_empty() {
            return Err(QueryError::EmptyTargetSystem);
        }
        // NOTE: PIXm §2:3.83.4.2.2.4, a target that is no absolute URI names no
        // domain the Manager can recognise, so it gets that refusal.
        let target = TargetSystem::new(system).map_err(|_not_a_uri| QueryError::TargetDomain)?;
        self.targets.push(target);
        Ok(())
    }

    fn format(&mut self, value: &str) -> Result<(), QueryError> {
        if self.format.is_some() {
            return Err(QueryError::SecondFormat);
        }
        self.format = Some(value.to_owned());
        Ok(())
    }

    fn finish(self) -> Result<PixQuery, QueryError> {
        Ok(PixQuery {
            source: self.source.ok_or(QueryError::NoSourceIdentifier)?,
            targets: self.targets,
            format: self.format,
        })
    }
}

/// Why a request is no `$ihe-pix` request the Manager answers.
///
/// No variant carries a value or a parameter name from the request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum QueryError {
    /// The request names no `sourceIdentifier` (§2:3.83.4.1.2.1).
    #[error("the request names no sourceIdentifier")]
    NoSourceIdentifier,
    /// The request names a second `sourceIdentifier`; the Consumer "shall
    /// provide exactly one" (§2:3.83.4.1.2.1).
    #[error("the request names more than one sourceIdentifier")]
    SecondSourceIdentifier,
    /// The `sourceIdentifier` is a token without a system: it "shall include
    /// both the Patient Identifier Domain … and the identifier value"
    /// (§2:3.83.4.1.2.1).
    #[error("the sourceIdentifier carries no assigning authority")]
    NoAssigningAuthority,
    /// The `sourceIdentifier` carries no identifier value.
    #[error("the sourceIdentifier carries no identifier value")]
    EmptyValue,
    /// The `sourceIdentifier`'s system is not an absolute URI, so it names no
    /// domain the Manager recognises (§2:3.83.4.2.2.3).
    #[error("the sourceIdentifier's assigning authority is not an absolute URI")]
    SourceDomain,
    /// A `targetSystem` is empty.
    #[error("a targetSystem is empty")]
    EmptyTargetSystem,
    /// A `targetSystem` is not an absolute URI, so it names no domain the
    /// Manager recognises (§2:3.83.4.2.2.4).
    #[error("a targetSystem is not an absolute URI")]
    TargetDomain,
    /// The request names `_format` twice.
    #[error("the request names _format more than once")]
    SecondFormat,
    /// The request names a parameter `$ihe-pix` does not define (the
    /// `$ihe-pix` `OperationDefinition`; FHIR R4 http.html for `_format` and
    /// `_pretty`).
    #[error("the request names a parameter $ihe-pix does not define")]
    UnknownParameter,
    /// A posted parameter is no `valueString`, or carries a resource, parts
    /// or a modifier extension; with no index, the `Parameters` resource
    /// carries `implicitRules` (the Query Parameters In profile).
    #[error("a posted parameter does not hold to the Query Parameters In profile")]
    Shape {
        /// The parameter's position, when one parameter is at fault.
        index: Option<usize>,
    },
    /// The body is no FHIR resource.
    #[error("the body is no FHIR R4 resource")]
    Body(#[source] BodyError),
    /// The body is a resource other than `Parameters`.
    #[error("the body is not a Parameters resource")]
    NotParameters,
}

impl QueryError {
    /// Returns the answer to send: the two domain failures as the
    /// transaction prescribes them (§2:3.83.4.2.2.3, §2:3.83.4.2.2.4), and
    /// every other failure a `400` whose issue names the fault.
    #[must_use]
    pub fn refusal(&self) -> Answer<OperationOutcome> {
        let bad = |code, diagnostics| {
            Answer::outcome(StatusCode::BAD_REQUEST, Severity::Error, code, diagnostics)
        };
        match self {
            Self::SourceDomain => Refusal::SourceDomainNotRecognized.answer(),
            Self::TargetDomain => Refusal::TargetDomainNotRecognized.answer(),
            Self::NoSourceIdentifier => bad(IssueType::Required, "sourceIdentifier is required"),
            Self::SecondSourceIdentifier => bad(
                IssueType::Invalid,
                "exactly one sourceIdentifier is allowed",
            ),
            Self::NoAssigningAuthority => bad(
                IssueType::Invalid,
                "sourceIdentifier shall include the Assigning Authority",
            ),
            Self::EmptyValue => bad(
                IssueType::Invalid,
                "sourceIdentifier shall include the identifier value",
            ),
            Self::EmptyTargetSystem => bad(IssueType::Invalid, "targetSystem is empty"),
            Self::SecondFormat => bad(IssueType::Invalid, "_format is allowed once"),
            Self::UnknownParameter => bad(
                IssueType::NotSupported,
                "a parameter $ihe-pix does not define",
            ),
            Self::Shape { .. } => bad(
                IssueType::Structure,
                "the Parameters do not hold to the Query Parameters In profile",
            ),
            Self::Body(_) => bad(IssueType::Structure, "the body is no FHIR R4 resource"),
            Self::NotParameters => bad(IssueType::Invalid, "the body is not a Parameters resource"),
        }
    }
}

#[cfg(test)]
mod tests {
    use http::StatusCode;
    use secrecy::ExposeSecret;

    use super::{PixQuery, QueryError};
    use crate::outcome::IssueType;
    use crate::pixm::identifier::TargetSystem;

    #[test]
    fn the_get_query_is_read_with_its_escapes() {
        let query = PixQuery::from_query(
            "sourceIdentifier=urn:oid:2.999.1%7Cx%5C%7C1&targetSystem=urn:oid:2.999.2&targetSystem=urn:oid:2.999.3&_format=json&_pretty=true",
        )
        .expect("an ITI-83 query");
        assert_eq!(query.source().system(), "urn:oid:2.999.1");
        assert_eq!(
            query.source().value().expose_secret(),
            "x|1",
            "the escaped bar"
        );
        let targets: Vec<&str> = query.targets().iter().map(TargetSystem::as_str).collect();
        assert_eq!(targets, ["urn:oid:2.999.2", "urn:oid:2.999.3"]);
        assert_eq!(query.format(), Some("json"));
    }

    #[test]
    fn each_refusal_has_its_status_and_issue() {
        for (query, error, status, code) in [
            (
                "targetSystem=urn:oid:2.999.2",
                QueryError::NoSourceIdentifier,
                StatusCode::BAD_REQUEST,
                IssueType::Required,
            ),
            (
                "sourceIdentifier=urn:oid:2.999.1%7C1&sourceIdentifier=urn:oid:2.999.1%7C2",
                QueryError::SecondSourceIdentifier,
                StatusCode::BAD_REQUEST,
                IssueType::Invalid,
            ),
            (
                "sourceIdentifier=IHERED-994",
                QueryError::NoAssigningAuthority,
                StatusCode::BAD_REQUEST,
                IssueType::Invalid,
            ),
            (
                "sourceIdentifier=%7CIHERED-994",
                QueryError::NoAssigningAuthority,
                StatusCode::BAD_REQUEST,
                IssueType::Invalid,
            ),
            (
                "sourceIdentifier=urn:oid:2.999.1%7C",
                QueryError::EmptyValue,
                StatusCode::BAD_REQUEST,
                IssueType::Invalid,
            ),
            (
                "sourceIdentifier=not%20a%20uri%7C1",
                QueryError::SourceDomain,
                StatusCode::BAD_REQUEST,
                IssueType::CodeInvalid,
            ),
            (
                "sourceIdentifier=urn:oid:2.999.1%7C1&targetSystem=2.999.2",
                QueryError::TargetDomain,
                StatusCode::FORBIDDEN,
                IssueType::CodeInvalid,
            ),
            (
                "sourceIdentifier=urn:oid:2.999.1%7C1&identifier=x",
                QueryError::UnknownParameter,
                StatusCode::BAD_REQUEST,
                IssueType::NotSupported,
            ),
        ] {
            let refused = PixQuery::from_query(query).expect_err(query);
            assert_eq!(refused, error, "{query}");
            let answer = refused.refusal();
            assert_eq!(answer.status(), status, "{query}");
            assert_eq!(answer.issue_type(), code, "{query}");
        }
    }
}
