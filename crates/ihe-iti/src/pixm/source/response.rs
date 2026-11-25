// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-104 responses, which the transaction takes from FHIR R4's
//! conditional update and delete (§2:3.104.4.1.4, §2:3.104.4.2.4,
//! §2:3.104.4.3.4).

use http::StatusCode;
use http::header::LOCATION;
use secrecy::SecretString;

use super::error::SourceError;
use super::{Fed, Receipt, Removed};
use crate::exchange;
use crate::outcome;
use crate::pixm::identifier::PatientReference;

/// The longest failure answer the client reads for its issues. An
/// `OperationOutcome` is a few kilobytes at most.
const LIMIT: usize = 1 << 16;

/// What the answer `response` to a conditional update means.
pub(super) async fn update(response: reqwest::Response) -> Result<Fed, SourceError> {
    let status = response.status();
    // NOTE: FHIR R4 http.html §3.1.0.4, a successful update answers 200 OK, or 201
    // Created when the interaction created the resource.
    if status == StatusCode::CREATED {
        return Ok(Fed::Created(receipt(&response)));
    }
    if status == StatusCode::OK {
        return Ok(Fed::Updated(receipt(&response)));
    }
    Err(refusal(response).await)
}

/// What the answer `response` to a conditional delete means.
pub(super) async fn delete(response: reqwest::Response) -> Result<Removed, SourceError> {
    let status = response.status();
    // NOTE: FHIR R4 http.html §3.1.0.7, a delete, also of nothing, answers 200 OK or
    // 204 No Content, or 202 Accepted when the server does not commit to the outcome.
    if status == StatusCode::OK || status == StatusCode::NO_CONTENT {
        return Ok(Removed::Deleted);
    }
    if status == StatusCode::ACCEPTED {
        return Ok(Removed::Accepted);
    }
    Err(refusal(response).await)
}

/// The `Location` of a successful update, when the Manager names one.
fn receipt(response: &reqwest::Response) -> Receipt {
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .filter(|location| !location.is_empty())
        .map(|location| PatientReference::read(SecretString::from(location)));
    Receipt { location }
}

/// The error of a failure answer, read for its `OperationOutcome`'s issue
/// types; the status decides.
async fn refusal(response: reqwest::Response) -> SourceError {
    let status = response.status();
    let issues = match exchange::read(response, LIMIT, |_status| SourceError::Timeout).await {
        Ok(answer) => outcome::issues(answer.media.as_deref(), &answer.body),
        // NOTE: no specification governs this: our own design, the status decides a
        // failure, so a body too long or cut off leaves only its issues unread.
        Err(_unread) => Vec::new(),
    };
    if status == StatusCode::PRECONDITION_FAILED {
        return SourceError::NotSelective { issues };
    }
    SourceError::Rejected { status, issues }
}
