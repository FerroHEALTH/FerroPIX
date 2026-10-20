// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-78 and ITI-119 responses: a `searchset` Bundle on success, an
//! `OperationOutcome` on failure (§2:3.78.4.2.2, §2:3.78.4.1.3,
//! §2:3.119.4.2.2, §2:3.119.4.1.3).

use fhir_types::codec::{DecodeError, Json, Object, Path, Value};
use fhir_types::r4::bundle::{Bundle, BundleEntrySearch, BundleLink};
use fhir_types::r4::extension::ExtensionValue;
use fhir_types::r4::operation_outcome::OperationOutcome;
use fhir_types::r4::resource::Resource;
use http::StatusCode;
use secrecy::SecretString;
use url::Url;

use crate::outcome::{self, IssueType};

use super::error::{self, Malformation, PdqmError};
use super::matches::{MatchGrade, MatchResult, MatchedPatient, Page, SearchResult};

/// The longest answer the client reads: one page of Patients.
// NOTE: no specification governs this: our own design, a bound on what one
// page costs to read and decode.
pub(super) const LIMIT: usize = 8 << 20;

/// Reads the answer's body, refusing one longer than [`LIMIT`].
pub(super) async fn body(mut response: reqwest::Response) -> Result<Vec<u8>, PdqmError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(error::transport)? {
        if body.len().saturating_add(chunk.len()) > LIMIT {
            return Err(Malformation::TooLarge { limit: LIMIT }.into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// What the answer with `status`, media type `media` and `body` means, for a
/// request to the FHIR base `base` whose query names an identifier domain when
/// `names_domain` holds.
pub(super) fn read(
    status: StatusCode,
    media: Option<&str>,
    body: &[u8],
    base: &Url,
    names_domain: bool,
) -> Result<SearchResult, PdqmError> {
    if status == StatusCode::OK {
        if !outcome::fhir_json(media) {
            return Err(Malformation::NotFhirJson.into());
        }
        let value = json(body)?;
        let object = value.as_object().ok_or(Malformation::NotAResource)?;
        if resource_type(object)? != "Bundle" {
            return Err(Malformation::UnexpectedResource.into());
        }
        return Ok(searchset(object, base)?);
    }
    let issues = outcome::issues(media, body);
    if status == StatusCode::NOT_FOUND && names_domain && issues.contains(&IssueType::NotFound) {
        return Err(PdqmError::DomainNotRecognized);
    }
    Err(PdqmError::Rejected { status, issues })
}

/// What the answer to an ITI-119 match with `status`, media type `media` and
/// `body` means (§2:3.119.4.1.3).
///
/// A failure answers with an `OperationOutcome` or a Bundle of them (Case 9);
/// the issue types of either are kept.
pub(super) fn read_match(
    status: StatusCode,
    media: Option<&str>,
    body: &[u8],
) -> Result<MatchResult, PdqmError> {
    if status == StatusCode::OK {
        if !outcome::fhir_json(media) {
            return Err(Malformation::NotFhirJson.into());
        }
        let value = json(body)?;
        let object = value.as_object().ok_or(Malformation::NotAResource)?;
        if resource_type(object)? != "Bundle" {
            return Err(Malformation::UnexpectedResource.into());
        }
        return Ok(matchset(object)?);
    }
    let mut issues = outcome::issues(media, body);
    if issues.is_empty() {
        issues = bundled_issues(media, body);
    }
    Err(PdqmError::Rejected { status, issues })
}

/// The issue types of every `OperationOutcome` in a failure answer that is a
/// Bundle of them (§2:3.119.4.1.3, Case 9), or none when it is not one.
fn bundled_issues(media: Option<&str>, body: &[u8]) -> Vec<IssueType> {
    if !outcome::fhir_json(media) {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    // NOTE: FHIR R4 http.html, a failure body is read for its issue codes only;
    // the status decides, and a body that is no Bundle of outcomes adds nothing.
    let Ok(bundle) = Bundle::from_json(object, &mut Path::root("Bundle")) else {
        return Vec::new();
    };
    bundle
        .entry
        .iter()
        .filter_map(|entry| match &entry.resource {
            Some(Resource::OperationOutcome(found)) => Some(outcome::of(found)),
            _ => None,
        })
        .flatten()
        .collect()
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

/// The page a `searchset` Bundle holds, held to the Query Patient Resource
/// Response Message profile: `type` `searchset`, a `total`, and a `fullUrl` on
/// every entry.
fn searchset(object: &Object, base: &Url) -> Result<SearchResult, Malformation> {
    let bundle = Bundle::from_json(object, &mut Path::root("Bundle")).map_err(decode)?;
    if bundle.r#type.value.as_deref() != Some("searchset") {
        return Err(Malformation::NotSearchset);
    }
    let total = bundle
        .total
        .as_ref()
        .and_then(|total| total.value)
        .ok_or(Malformation::NoTotal)?;
    let next = next(&bundle.link, base)?;
    let mut patients = Vec::new();
    let mut warnings = Vec::new();
    for (index, entry) in bundle.entry.into_iter().enumerate() {
        let full_url = entry
            .full_url
            .and_then(|full_url| full_url.value)
            .filter(|full_url| !full_url.is_empty())
            .ok_or(Malformation::NoFullUrl { index })?;
        match entry.resource {
            Some(Resource::Patient(patient)) => {
                let search = entry.search.as_ref();
                patients.push(MatchedPatient::new(
                    SecretString::from(full_url),
                    patient,
                    score(index, search)?,
                    grade(index, search)?,
                ));
            }
            Some(Resource::OperationOutcome(found)) => warnings.extend(outcome::of(&found)),
            Some(_) => return Err(Malformation::UnexpectedEntry { index }),
            None => return Err(Malformation::NoResource { index }),
        }
    }
    if u32::try_from(patients.len()).map_or(true, |matched| matched > total) {
        return Err(Malformation::TotalBelowMatches);
    }
    Ok(SearchResult::new(total, patients, warnings, next))
}

/// The `next` link, resolved against the FHIR base (FHIR R4 paging,
/// <http://hl7.org/fhir/R4/http.html#paging>).
fn next(links: &[BundleLink], base: &Url) -> Result<Option<Page>, Malformation> {
    let Some(link) = links
        .iter()
        .find(|link| link.relation.value.as_deref() == Some("next"))
    else {
        return Ok(None);
    };
    let url = link.url.value.as_deref().ok_or(Malformation::NextLink)?;
    base.join(url)
        .map(|url| Some(Page::new(url)))
        .map_err(|_unparsable| Malformation::NextLink)
}

/// The matches a `$match` Bundle holds, held to the PDQm Match Output Bundle
/// profile: `type` `searchset`, and on every Patient entry a `fullUrl`, the
/// search mode `match`, a score between 0 and 1 and a `match-grade`
/// (§2:3.119.4.2.2.4). An `OperationOutcome` entry is a warning, never an
/// error (§2:3.119.4.1.3, Cases 7 and 10).
fn matchset(object: &Object) -> Result<MatchResult, Malformation> {
    let bundle = Bundle::from_json(object, &mut Path::root("Bundle")).map_err(decode)?;
    if bundle.r#type.value.as_deref() != Some("searchset") {
        return Err(Malformation::NotSearchset);
    }
    let mut patients = Vec::new();
    let mut warnings = Vec::new();
    for (index, entry) in bundle.entry.into_iter().enumerate() {
        let full_url = entry
            .full_url
            .and_then(|full_url| full_url.value)
            .filter(|full_url| !full_url.is_empty())
            .ok_or(Malformation::NoFullUrl { index })?;
        match entry.resource {
            Some(Resource::Patient(patient)) => {
                let search = entry.search.as_ref();
                if search
                    .and_then(|search| search.mode.as_ref())
                    .and_then(|mode| mode.value.as_deref())
                    != Some("match")
                {
                    return Err(Malformation::NotMatchMode { index });
                }
                let score = score(index, search)?
                    .filter(|score| (0.0..=1.0).contains(score))
                    .ok_or(Malformation::NoScore { index })?;
                let grade = grade(index, search)?.ok_or(Malformation::NoMatchGrade { index })?;
                patients.push(MatchedPatient::new(
                    SecretString::from(full_url),
                    patient,
                    Some(score),
                    Some(grade),
                ));
            }
            Some(Resource::OperationOutcome(found)) => {
                if failed(&found) {
                    return Err(Malformation::ErrorOutcome { index });
                }
                warnings.extend(outcome::of(&found));
            }
            Some(_) => return Err(Malformation::UnexpectedEntry { index }),
            None => return Err(Malformation::NoResource { index }),
        }
    }
    Ok(MatchResult::new(patients, warnings))
}

/// Whether `found` holds an issue of `error` or `fatal` severity.
fn failed(found: &OperationOutcome) -> bool {
    found
        .issue
        .iter()
        .any(|issue| matches!(issue.severity.value.as_deref(), Some("error" | "fatal")))
}

/// The entry's `search.score`, a finite decimal (§2:3.78.4.2.2.5).
fn score(index: usize, search: Option<&BundleEntrySearch>) -> Result<Option<f64>, Malformation> {
    let Some(text) = search
        .and_then(|search| search.score.as_ref())
        .and_then(|score| score.value.as_deref())
    else {
        return Ok(None);
    };
    match text.parse::<f64>() {
        Ok(score) if score.is_finite() => Ok(Some(score)),
        _ => Err(Malformation::Score { index }),
    }
}

/// The entry's `match-grade` extension on `search`
/// (<http://hl7.org/fhir/R4/extension-match-grade.html>).
fn grade(
    index: usize,
    search: Option<&BundleEntrySearch>,
) -> Result<Option<MatchGrade>, Malformation> {
    let Some(extension) = search.and_then(|search| {
        search
            .extension
            .iter()
            .find(|extension| extension.url == MatchGrade::EXTENSION)
    }) else {
        return Ok(None);
    };
    let Some(ExtensionValue::Code(code)) = &extension.value else {
        return Err(Malformation::MatchGrade { index });
    };
    code.value
        .as_deref()
        .and_then(MatchGrade::from_code)
        .map(Some)
        .ok_or(Malformation::MatchGrade { index })
}
