// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! An answer a server sends: an HTTP status and one FHIR R4 resource,
//! encoded in the format negotiated (FHIR R4 http.html).

use std::fmt;

use fhir_types::codec::{EncodeError, Json, Object};
use fhir_types::r4::bundle::Bundle;
use fhir_types::r4::operation_outcome::{OperationOutcome, OperationOutcomeIssue};
use fhir_types::r4::parameters::Parameters;
use http::StatusCode;

use super::format::Format;
use crate::outcome::IssueType;

/// A resource an [`Answer`] carries.
///
/// The trait is sealed: the resources are the ones the profiles' server sides
/// answer with.
pub trait Resource: sealed::Sealed + Json {
    /// The resource type, as `resourceType` names it.
    const RESOURCE_TYPE: &'static str;
}

mod sealed {
    pub trait Sealed {
        /// The resource as compact FHIR JSON.
        fn json(&self) -> serde_json::Result<Vec<u8>>;
    }
}

macro_rules! resource {
    ($type:ty, $name:literal) => {
        impl sealed::Sealed for $type {
            fn json(&self) -> serde_json::Result<Vec<u8>> {
                serde_json::to_vec(self)
            }
        }
        impl Resource for $type {
            const RESOURCE_TYPE: &'static str = $name;
        }
    };
}

resource!(Parameters, "Parameters");
resource!(Bundle, "Bundle");
resource!(OperationOutcome, "OperationOutcome");
#[cfg(feature = "pdqm-server")]
resource!(fhir_types::r4::patient::Patient, "Patient");

/// One answer: its HTTP status and its resource.
///
/// The resource may hold identifiers and demographics, so `Debug` shows the
/// status and the resource type only, and the type has no `Display`.
#[derive(Clone)]
pub struct Answer<T> {
    status: StatusCode,
    resource: T,
}

impl<T: Resource> Answer<T> {
    pub(crate) fn new(status: StatusCode, resource: T) -> Self {
        Self { status, resource }
    }

    /// Returns the HTTP status.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns the resource.
    #[must_use]
    pub fn resource(&self) -> &T {
        &self.resource
    }

    /// Returns the resource, by value.
    #[must_use]
    pub fn into_resource(self) -> T {
        self.resource
    }

    /// Returns the resource encoded in `format`, the body of the answer, whose
    /// media type is [`Format::media_type`].
    ///
    /// JSON is written compact, its properties in the codepoint order
    /// `fhir-types` writes; XML in the order of each element's definition
    /// (FHIR R4 xml.html).
    ///
    /// # Errors
    /// [`WriteError`] when the resource holds a value with no form in
    /// `format`, such as a decimal that is not a number.
    pub fn encode(&self, format: Format) -> Result<Vec<u8>, WriteError> {
        // NOTE: CLAUDE.md, no identifying data in an error: the encoder's reason can
        // quote a value of the resource, so the error names the format alone.
        match format {
            Format::Json => {
                sealed::Sealed::json(&self.resource).map_err(|_quoted| WriteError::Json)
            }
            Format::Xml => {
                let object: Object = self
                    .resource
                    .to_json()
                    .map_err(|_quoted: EncodeError| WriteError::Xml)?;
                fhir_types::xml::to_xml(&fhir_types::r4::schema::SCHEMAS, &object)
                    .map(String::into_bytes)
                    .map_err(|_quoted| WriteError::Xml)
            }
        }
    }
}

impl Answer<OperationOutcome> {
    /// Returns an `OperationOutcome` answer with one issue of `severity` and
    /// type `code`, whose `diagnostics` is `diagnostics`.
    ///
    /// The diagnostics are static text, so an outcome never quotes a request.
    #[must_use]
    pub fn outcome(
        status: StatusCode,
        severity: Severity,
        code: IssueType,
        diagnostics: &'static str,
    ) -> Self {
        let issue = OperationOutcomeIssue {
            severity: severity.code().into(),
            code: code.to_string().into(),
            diagnostics: Some(diagnostics.into()),
            ..OperationOutcomeIssue::default()
        };
        Self::new(
            status,
            OperationOutcome {
                issue: vec![issue],
                ..OperationOutcome::default()
            },
        )
    }

    /// Returns the type of the outcome's first issue.
    #[must_use]
    pub fn issue_type(&self) -> IssueType {
        crate::outcome::of(&self.resource)
            .first()
            .copied()
            .unwrap_or(IssueType::Unrecognized)
    }
}

impl<T: Resource> fmt::Debug for Answer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Answer")
            .field("status", &self.status.as_u16())
            .field("resource", &T::RESOURCE_TYPE)
            .finish()
    }
}

/// The severity of an `OperationOutcome` issue (FHIR R4 `issue-severity`, a
/// required binding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Severity {
    /// `fatal`
    Fatal,
    /// `error`
    Error,
    /// `warning`
    Warning,
    /// `information`
    Information,
}

impl Severity {
    fn code(self) -> &'static str {
        match self {
            Self::Fatal => "fatal",
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Information => "information",
        }
    }
}

/// An answer could not be encoded.
///
/// No variant carries the encoder's reason, which may quote a value of the
/// resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WriteError {
    /// The resource has no FHIR JSON form.
    #[error("the answer has no FHIR JSON form")]
    Json,
    /// The resource has no FHIR XML form.
    #[error("the answer has no FHIR XML form")]
    Xml,
}

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use super::{Answer, Severity};
    use crate::outcome::IssueType;
    use crate::server::format::Format;

    #[test]
    fn an_outcome_is_written_in_both_encodings() {
        let answer = Answer::outcome(
            StatusCode::NOT_FOUND,
            Severity::Error,
            IssueType::NotFound,
            "sourceIdentifier Patient Identifier not found",
        );
        assert_eq!(answer.issue_type(), IssueType::NotFound, "the issue type");
        let json = answer.encode(Format::Json).expect("JSON");
        assert_eq!(
            String::from_utf8(json).expect("UTF-8"),
            r#"{"issue":[{"code":"not-found","diagnostics":"sourceIdentifier Patient Identifier not found","severity":"error"}],"resourceType":"OperationOutcome"}"#,
            "compact"
        );
        let xml = String::from_utf8(answer.encode(Format::Xml).expect("XML")).expect("UTF-8");
        assert!(
            xml.starts_with(r#"<OperationOutcome xmlns="http://hl7.org/fhir">"#),
            "{xml}"
        );
        assert!(xml.contains(r#"<code value="not-found"/>"#), "{xml}");
        assert_eq!(
            format!("{answer:?}"),
            r#"Answer { status: 404, resource: "OperationOutcome" }"#,
            "Debug names the resource type only"
        );
    }
}
