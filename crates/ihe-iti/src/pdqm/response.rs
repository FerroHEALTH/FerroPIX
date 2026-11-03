// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-78 and ITI-119 responses: a `searchset` Bundle on success, an
//! `OperationOutcome` on failure (§2:3.78.4.2.2, §2:3.78.4.1.3,
//! §2:3.119.4.2.2, §2:3.119.4.1.3).

use fhir_types::codec::{Json, Object, Path};
use fhir_types::r4::bundle::{Bundle, BundleEntry, BundleEntrySearch};
use fhir_types::r4::extension::ExtensionValue;
use fhir_types::r4::operation_outcome::OperationOutcome;
use fhir_types::r4::patient::Patient;
use fhir_types::r4::resource::Resource;
use http::StatusCode;
use secrecy::SecretString;
use url::Url;

use crate::outcome::{self, IssueType, Unreadable};

use super::error::{Malformation, PdqmError};
use super::matches::{MatchGrade, MatchResult, MatchedPatient, Page, SearchResult};

/// The longest answer the client reads: one page of Patients.
// NOTE: no specification governs this: our own design, a bound on what one
// page costs to read and decode.
pub(super) const LIMIT: usize = 8 << 20;

/// The refusal of an answer longer than [`LIMIT`], whatever its status.
pub(super) fn too_large(_status: StatusCode) -> PdqmError {
    Malformation::from(Unreadable::TooLarge { limit: LIMIT }).into()
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
        return Ok(searchset(&bundle(media, body)?, base)?);
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
        return Ok(matchset(&bundle(media, body)?)?);
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
    let Some(object) = outcome::failure_object(media, body) else {
        return Vec::new();
    };
    // NOTE: FHIR R4 http.html, a failure body is read for its issue codes only;
    // the status decides, and a body that is no Bundle of outcomes adds nothing.
    let Ok(bundle) = Bundle::from_json(&object, &mut Path::root("Bundle")) else {
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

/// The members of the Bundle a success answer holds.
fn bundle(media: Option<&str>, body: &[u8]) -> Result<Object, Malformation> {
    let received = outcome::resource(media, body)?;
    if received.resource_type != "Bundle" {
        return Err(Malformation::UnexpectedResource);
    }
    Ok(received.object)
}

/// The `searchset` Bundle `object` holds: both the Query Patient Resource
/// Response Message and the Match Output Bundle profile fix its `type`.
fn searchset_bundle(object: &Object) -> Result<Bundle, Malformation> {
    let bundle = Bundle::from_json(object, &mut Path::root("Bundle")).map_err(Unreadable::from)?;
    if bundle.r#type.value.as_deref() != Some("searchset") {
        return Err(Malformation::NotSearchset);
    }
    Ok(bundle)
}

/// The Patients and the warnings of a Bundle's `entries`, read in order so
/// the first departure in the Bundle is the one reported: each Patient entry
/// at its position, with its `fullUrl` and `search`, by `patient`, and each
/// `OperationOutcome` entry at its position by `held` before its issue
/// types are kept. Both profiles require a `fullUrl` on every entry and
/// answer with no `_include`.
fn read_entries(
    entries: Vec<BundleEntry>,
    mut patient: impl FnMut(
        usize,
        SecretString,
        Box<Patient>,
        Option<&BundleEntrySearch>,
    ) -> Result<MatchedPatient, Malformation>,
    held: impl Fn(usize, &OperationOutcome) -> Result<(), Malformation>,
) -> Result<(Vec<MatchedPatient>, Vec<IssueType>), Malformation> {
    let mut patients = Vec::new();
    let mut warnings = Vec::new();
    for (index, entry) in entries.into_iter().enumerate() {
        let full_url = entry
            .full_url
            .as_ref()
            .and_then(|full_url| full_url.value.as_deref())
            .filter(|full_url| !full_url.is_empty())
            .map(SecretString::from)
            .ok_or(Malformation::NoFullUrl { index })?;
        match entry.resource {
            Some(Resource::Patient(found)) => {
                patients.push(patient(index, full_url, found, entry.search.as_ref())?);
            }
            Some(Resource::OperationOutcome(found)) => {
                held(index, &found)?;
                warnings.extend(outcome::of(&found));
            }
            Some(_) => return Err(Malformation::UnexpectedEntry { index }),
            None => return Err(Malformation::NoResource { index }),
        }
    }
    Ok((patients, warnings))
}

/// The page a `searchset` Bundle holds, held to the Query Patient Resource
/// Response Message profile: `type` `searchset`, a `total`, and a `fullUrl` on
/// every entry.
fn searchset(object: &Object, base: &Url) -> Result<SearchResult, Malformation> {
    let bundle = searchset_bundle(object)?;
    let total = bundle
        .total
        .as_ref()
        .and_then(|total| total.value)
        .ok_or(Malformation::NoTotal)?;
    let next = crate::bundle::next(&bundle.link, base)
        .map_err(|_bad| Malformation::NextLink)?
        .map(Page::new);
    let (patients, warnings) = read_entries(
        bundle.entry,
        |index, full_url, patient, search| {
            Ok(MatchedPatient::new(
                full_url,
                patient,
                score(index, search)?,
                grade(index, search)?,
            ))
        },
        |_, _| Ok(()),
    )?;
    if u32::try_from(patients.len()).map_or(true, |matched| matched > total) {
        return Err(Malformation::TotalBelowMatches);
    }
    Ok(SearchResult::new(total, patients, warnings, next))
}

/// The matches a `$match` Bundle holds, held to the PDQm Match Output Bundle
/// profile: `type` `searchset`, and on every Patient entry a `fullUrl`, the
/// search mode `match`, a score between 0 and 1 and a `match-grade`
/// (§2:3.119.4.2.2.4). An `OperationOutcome` entry is a warning, never an
/// error (§2:3.119.4.1.3, Cases 7 and 10).
fn matchset(object: &Object) -> Result<MatchResult, Malformation> {
    let bundle = searchset_bundle(object)?;
    let (patients, warnings) = read_entries(
        bundle.entry,
        |index, full_url, patient, search| {
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
            Ok(MatchedPatient::new(
                full_url,
                patient,
                Some(score),
                Some(grade),
            ))
        },
        |index, found| {
            if failed(found) {
                Err(Malformation::ErrorOutcome { index })
            } else {
                Ok(())
            }
        },
    )?;
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
