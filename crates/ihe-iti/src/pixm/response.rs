// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-83 response: a `Parameters` resource on success, an
//! `OperationOutcome` on failure (§2:3.83.4.2.2).

use fhir_types::codec::{DecodeError, Json, Object, Path, Value};
use fhir_types::r4::bundle::Bundle;
use fhir_types::r4::operation_outcome::OperationOutcome;
use fhir_types::r4::parameters::{Parameters, ParametersParameter, ParametersParameterValue};
use http::StatusCode;
use secrecy::{ExposeSecret, SecretString};

use super::error::{self, IssueType, Malformation, PixmError};
use super::identifier::{
    CrossReference, CrossReferences, PatientReference, SourceIdentifier, TargetIdentifier,
    TargetSystem,
};

/// The longest answer the client reads. An ITI-83 answer is one Parameters
/// resource with an identifier per domain, a few kilobytes at most.
pub(super) const LIMIT: usize = 1 << 20;

/// Reads the answer's body, refusing one longer than [`LIMIT`].
pub(super) async fn body(mut response: reqwest::Response) -> Result<Vec<u8>, PixmError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(error::transport)? {
        if body.len().saturating_add(chunk.len()) > LIMIT {
            return Err(Malformation::TooLarge { limit: LIMIT }.into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// What the answer with `status`, media type `media` and `body` means for the
/// request that asked about `source` in the domains `targets`.
pub(super) fn read(
    status: StatusCode,
    media: Option<&str>,
    body: &[u8],
    source: &SourceIdentifier,
    targets: &[TargetSystem],
) -> Result<CrossReference, PixmError> {
    if status == StatusCode::OK {
        if !fhir_json(media) {
            return Err(Malformation::NotFhirJson.into());
        }
        let value = json(body)?;
        let object = value.as_object().ok_or(Malformation::NotAResource)?;
        return match resource_type(object)? {
            "Parameters" => Ok(CrossReference::Matched(matched(object, source, targets)?)),
            "Bundle" => post_merge(object),
            _ => Err(Malformation::UnexpectedResource.into()),
        };
    }
    let issues = issues(media, body);
    if status == StatusCode::NOT_FOUND && issues.contains(&IssueType::NotFound) {
        return Ok(CrossReference::SourceNotFound);
    }
    if status == StatusCode::BAD_REQUEST && issues.contains(&IssueType::CodeInvalid) {
        return Err(PixmError::SourceDomainNotRecognized);
    }
    if status == StatusCode::FORBIDDEN && issues.contains(&IssueType::CodeInvalid) {
        return Err(PixmError::TargetDomainNotRecognized);
    }
    Err(PixmError::Rejected { status, issues })
}

/// Whether `media` is FHIR JSON (ITI TF-2 Appendix Z.6), or plain JSON, which
/// FHIR R4 accepts for the same content (<http://hl7.org/fhir/R4/http.html#mime-type>).
fn fhir_json(media: Option<&str>) -> bool {
    media
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|essence| {
            essence.eq_ignore_ascii_case("application/fhir+json")
                || essence.eq_ignore_ascii_case("application/json")
        })
}

fn json(body: &[u8]) -> Result<Value, Malformation> {
    serde_json::from_slice(body).map_err(|error| Malformation::NotJson {
        line: error.line(),
        column: error.column(),
    })
}

fn resource_type(object: &Object) -> Result<&str, Malformation> {
    object
        .get("resourceType")
        .and_then(Value::as_str)
        .ok_or(Malformation::NotAResource)
}

fn decode(error: DecodeError) -> Malformation {
    Malformation::Decode { kind: error.kind }
}

/// The identifiers of a success answer, held to the `$ihe-pix` out parameters
/// and to the request (§2:3.83.4.2.2.1).
fn matched(
    object: &Object,
    source: &SourceIdentifier,
    targets: &[TargetSystem],
) -> Result<CrossReferences, Malformation> {
    let parameters =
        Parameters::from_json(object, &mut Path::root("Parameters")).map_err(decode)?;
    let mut identifiers = Vec::new();
    let mut patients = Vec::new();
    for (index, parameter) in parameters.parameter.iter().enumerate() {
        if parameter.resource.is_some()
            || !parameter.part.is_empty()
            || !parameter.modifier_extension.is_empty()
        {
            return Err(Malformation::UnexpectedShape { index });
        }
        match parameter.name.value.as_deref() {
            Some("targetIdentifier") => {
                identifiers.push(target_identifier(index, parameter, source, targets)?);
            }
            Some("targetId") => patients.push(patient(index, parameter)?),
            _ => return Err(Malformation::UnexpectedParameter { index }),
        }
    }
    Ok(CrossReferences::new(identifiers, patients))
}

fn target_identifier(
    index: usize,
    parameter: &ParametersParameter,
    source: &SourceIdentifier,
    targets: &[TargetSystem],
) -> Result<TargetIdentifier, Malformation> {
    let Some(ParametersParameterValue::Identifier(identifier)) = &parameter.value else {
        return Err(Malformation::NotAnIdentifier { index });
    };
    let system = identifier
        .system
        .as_ref()
        .and_then(|system| system.value.as_deref())
        .filter(|system| !system.is_empty())
        .ok_or(Malformation::NoAssigningAuthority { index })?;
    let value = identifier
        .value
        .as_ref()
        .and_then(|value| value.value.as_deref())
        .filter(|value| !value.is_empty())
        .ok_or(Malformation::NoIdentifierValue { index })?;
    if !targets.is_empty() && !targets.iter().any(|target| target.as_str() == system) {
        return Err(Malformation::UnaskedDomain { index });
    }
    if system == source.system() && value == source.value().expose_secret() {
        return Err(Malformation::SourceEchoed { index });
    }
    Ok(TargetIdentifier::new(
        system.to_owned(),
        SecretString::from(value),
    ))
}

fn patient(
    index: usize,
    parameter: &ParametersParameter,
) -> Result<PatientReference, Malformation> {
    let Some(ParametersParameterValue::Reference(reference)) = &parameter.value else {
        return Err(Malformation::NoReference { index });
    };
    reference
        .reference
        .as_ref()
        .and_then(|reference| reference.value.as_deref())
        .filter(|reference| !reference.is_empty())
        .map(|reference| PatientReference::new(SecretString::from(reference)))
        .ok_or(Malformation::NoReference { index })
}

/// The `200` with a Bundle that the profile answers once the patient is
/// deprecated or deleted: no Patient in it, so the source is not found
/// (§2:3.83.4.2.2.5).
fn post_merge(object: &Object) -> Result<CrossReference, PixmError> {
    let bundle = Bundle::from_json(object, &mut Path::root("Bundle")).map_err(decode)?;
    if bundle.entry.is_empty() {
        Ok(CrossReference::SourceNotFound)
    } else {
        Err(Malformation::BundleWithEntries.into())
    }
}

/// The issue types of a failure answer's `OperationOutcome`, or none when the
/// body is not one (§2:3.83.4.2.2).
fn issues(media: Option<&str>, body: &[u8]) -> Vec<IssueType> {
    if !fhir_json(media) {
        return Vec::new();
    }
    let Ok(value) = json(body) else {
        return Vec::new();
    };
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    if resource_type(object) != Ok("OperationOutcome") {
        return Vec::new();
    }
    // NOTE: §2:3.83.4.2.2, a failure body that is no `OperationOutcome` is a
    // status alone; the status decides, and an undecodable outcome adds nothing.
    let Ok(outcome) = OperationOutcome::from_json(object, &mut Path::root("OperationOutcome"))
    else {
        return Vec::new();
    };
    outcome
        .issue
        .iter()
        .map(|issue| {
            issue
                .code
                .value
                .as_deref()
                .map_or(IssueType::Unrecognized, IssueType::from_code)
        })
        .collect()
}
