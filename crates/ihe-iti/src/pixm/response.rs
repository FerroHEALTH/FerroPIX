// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-83 response: a `Parameters` resource on success, an
//! `OperationOutcome` on failure (§2:3.83.4.2.2).

use fhir_types::codec::{Json, Object, Path};
use fhir_types::r4::bundle::Bundle;
use fhir_types::r4::parameters::{Parameters, ParametersParameter, ParametersParameterValue};
use http::StatusCode;
use secrecy::{ExposeSecret, SecretString};

use crate::outcome::{self, IssueType, Unreadable};

use super::error::{Malformation, PixmError};
use super::identifier::{
    CrossReference, CrossReferences, PatientReference, SourceIdentifier, TargetIdentifier,
    TargetSystem,
};

/// The longest answer the client reads. An ITI-83 answer is one Parameters
/// resource with an identifier per domain, a few kilobytes at most.
pub(super) const LIMIT: usize = 1 << 20;

/// The refusal of an answer longer than [`LIMIT`], whatever its status.
pub(super) fn too_large(_status: StatusCode) -> PixmError {
    Malformation::from(Unreadable::TooLarge { limit: LIMIT }).into()
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
        let received = outcome::resource(media, body).map_err(Malformation::from)?;
        let object = &received.object;
        return match received.resource_type.as_str() {
            "Parameters" => Ok(CrossReference::Matched(matched(object, source, targets)?)),
            "Bundle" => post_merge(object),
            _ => Err(Malformation::UnexpectedResource.into()),
        };
    }
    let issues = outcome::issues(media, body);
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

/// The identifiers of a success answer, held to the `$ihe-pix` out parameters
/// and to the request (§2:3.83.4.2.2.1).
fn matched(
    object: &Object,
    source: &SourceIdentifier,
    targets: &[TargetSystem],
) -> Result<CrossReferences, Malformation> {
    let parameters =
        Parameters::from_json(object, &mut Path::root("Parameters")).map_err(Unreadable::from)?;
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
    Ok(TargetIdentifier::read(
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
        .map(|reference| PatientReference::read(SecretString::from(reference)))
        .ok_or(Malformation::NoReference { index })
}

/// The `200` with a Bundle that the profile answers once the patient is
/// deprecated or deleted: no Patient in it, so the source is not found
/// (§2:3.83.4.2.2.5).
fn post_merge(object: &Object) -> Result<CrossReference, PixmError> {
    let bundle = Bundle::from_json(object, &mut Path::root("Bundle"))
        .map_err(Unreadable::from)
        .map_err(Malformation::from)?;
    if bundle.entry.is_empty() {
        Ok(CrossReference::SourceNotFound)
    } else {
        Err(Malformation::BundleWithEntries.into())
    }
}
