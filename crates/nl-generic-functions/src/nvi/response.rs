// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! One page of a Localization Service answer: a `searchset` Bundle of
//! localization records on success (FHIR R4 search, the
//! `nl-gf-localization-documentreference` profile).

use std::collections::BTreeSet;

use fhir_types::codec::{Json, Object, Path, Value};
use fhir_types::r4::bundle::Bundle;
use fhir_types::r4::document_reference::DocumentReference;
use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::resource::Resource;
use http::StatusCode;
use secrecy::ExposeSecret;

use super::error::{self, Malformation, NviError};
use super::{LOINC_SYSTEM, PATIENT_DATA_TYPE};
use crate::identification::{PSEUDO_BSN_SYSTEM, PseudoBsn, URA_SYSTEM, Ura};

/// The longest page the client reads (no specification governs this: our
/// own design).
pub(super) const LIMIT: usize = 1 << 20;

/// The custodians one page names, and its `next` link as written.
pub(super) struct Page {
    pub(super) custodians: BTreeSet<Ura>,
    pub(super) next: Option<String>,
}

/// Reads the answer's body, refusing one longer than [`LIMIT`].
pub(super) async fn body(mut response: reqwest::Response) -> Result<Vec<u8>, NviError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(error::transport)? {
        if body.len().saturating_add(chunk.len()) > LIMIT {
            return Err(Malformation::TooLarge { limit: LIMIT }.into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// What the answer with `status`, media type `media` and `body` holds for
/// the search about `patient`.
pub(super) fn page(
    status: StatusCode,
    media: Option<&str>,
    body: &[u8],
    patient: &PseudoBsn,
) -> Result<Page, NviError> {
    if status != StatusCode::OK {
        return Err(NviError::Rejected { status });
    }
    if !fhir_json(media) {
        return Err(Malformation::NotFhirJson.into());
    }
    Ok(searchset(body, patient)?)
}

/// The custodians of the records a `searchset` body holds, and its `next`
/// link.
fn searchset(body: &[u8], patient: &PseudoBsn) -> Result<Page, Malformation> {
    let value: Value = serde_json::from_slice(body).map_err(|error| Malformation::NotJson {
        line: error.line(),
        column: error.column(),
    })?;
    let object: &Object = value.as_object().ok_or(Malformation::NotABundle)?;
    if object.get("resourceType").and_then(Value::as_str) != Some("Bundle") {
        return Err(Malformation::NotABundle);
    }
    let bundle = Bundle::from_json(object, &mut Path::root("Bundle"))
        .map_err(|error| Malformation::Decode { kind: error.kind })?;
    if bundle.r#type.value.as_deref() != Some("searchset") {
        return Err(Malformation::NotASearchset);
    }
    let next = match bundle
        .link
        .iter()
        .find(|link| link.relation.value.as_deref() == Some("next"))
    {
        Some(link) => Some(link.url.value.clone().ok_or(Malformation::NextLink)?),
        None => None,
    };
    let mut custodians = BTreeSet::new();
    for (index, entry) in bundle.entry.into_iter().enumerate() {
        match entry.resource {
            Some(Resource::DocumentReference(record)) => {
                if let Some(custodian) = custodian(index, &record, patient)? {
                    custodians.insert(custodian);
                }
            }
            // NOTE: FHIR R4 search, search.mode `outcome`: an OperationOutcome
            // in a searchset tells about the search and is no record.
            Some(Resource::OperationOutcome(_)) => {}
            Some(_) => return Err(Malformation::UnexpectedEntry { index }),
            None => return Err(Malformation::NoResource { index }),
        }
    }
    Ok(Page { custodians, next })
}

/// The custodian a record at `index` localizes `patient` to, or `None` for a
/// record that no longer stands.
fn custodian(
    index: usize,
    record: &DocumentReference,
    patient: &PseudoBsn,
) -> Result<Option<Ura>, Malformation> {
    let subject = record
        .subject
        .as_ref()
        .and_then(|subject| subject.identifier.as_deref());
    let asked = subject.is_some_and(|identifier| {
        system(identifier) == Some(PSEUDO_BSN_SYSTEM)
            && value(identifier) == Some(patient.value().expose_secret())
    });
    if !asked {
        return Err(Malformation::OtherSubject { index });
    }
    let typed = record.r#type.as_ref().is_some_and(|concept| {
        concept.coding.iter().any(|coding| {
            coding.system.as_ref().and_then(|uri| uri.value.as_deref()) == Some(LOINC_SYSTEM)
                && coding.code.as_ref().and_then(|code| code.value.as_deref())
                    == Some(PATIENT_DATA_TYPE)
        })
    });
    if !typed {
        return Err(Malformation::OtherType { index });
    }
    let ura = record
        .custodian
        .as_ref()
        .and_then(|custodian| custodian.identifier.as_deref())
        .filter(|identifier| system(identifier) == Some(URA_SYSTEM))
        .and_then(value)
        .and_then(|value| Ura::new(value).ok())
        .ok_or(Malformation::NoCustodian { index })?;
    // NOTE: FHIR R4 DocumentReference.status: a `superseded` or
    // `entered-in-error` record no longer stands, so it localizes nothing.
    match record.status.value.as_deref() {
        Some("current") => Ok(Some(ura)),
        Some("superseded" | "entered-in-error") => Ok(None),
        _ => Err(Malformation::Status { index }),
    }
}

/// The system of `identifier`, when it has one.
fn system(identifier: &Identifier) -> Option<&str> {
    identifier
        .system
        .as_ref()
        .and_then(|uri| uri.value.as_deref())
}

/// The value of `identifier`, when it has one.
fn value(identifier: &Identifier) -> Option<&str> {
    identifier
        .value
        .as_ref()
        .and_then(|value| value.value.as_deref())
}

/// Whether `media` is FHIR JSON, or plain JSON, which FHIR R4 admits for a
/// JSON answer (<http://hl7.org/fhir/R4/http.html#mime-type>).
fn fhir_json(media: Option<&str>) -> bool {
    media
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|essence| {
            essence.eq_ignore_ascii_case("application/fhir+json")
                || essence.eq_ignore_ascii_case("application/json")
        })
}

#[cfg(test)]
mod tests {
    use super::fhir_json;

    #[test]
    fn fhir_json_and_plain_json_are_accepted_with_parameters() {
        assert!(fhir_json(Some("application/fhir+json; fhirVersion=4.0")));
        assert!(fhir_json(Some("Application/JSON")));
        assert!(!fhir_json(Some("text/html")));
        assert!(!fhir_json(None));
    }
}
