// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What every operator route shares on the wire: the JSON answer, the
//! refusals, the posted body, the query, the caller and the audit record.
//!
//! A refusal is an RFC 9457 problem of a [`crate::problem::Kind`] with a
//! fixed detail. A body or a query that does not read is refused without
//! the parser's message, which can quote the value it stopped at (RFC 9457
//! §5). No specification governs the operator API's documents: our own
//! design.

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::{ConnectInfo, MatchedPath};
use axum::response::Response;
use ferrohealth_smart::resource::Access;
use http::request::Parts;
use http::{StatusCode, header};
use ihe_atna::balp::Exchange;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::audit::Unrecorded;
use crate::audit::records::Call;
use crate::audit::reserve::{Pending, UNAUDITED};
use crate::fhir::outcome::body;
use crate::operator::{Operator, OperatorError};
use crate::problem::{self, Kind};
use ferrohealth_telemetry::request_log::RequestId;

/// The media type of every document the operator API reads and writes.
pub const JSON: &str = "application/json";

/// The detail of a body that is not the route's JSON document.
pub const NOT_THE_DOCUMENT: &str = "the request body is not the JSON document this route reads";

/// The detail of a query that does not read.
pub const NOT_THE_QUERY: &str =
    "the request query names a parameter or a value this route does not read";

/// The detail of a person id that names no person.
pub const NO_SUCH_PERSON: &str = "no person has this id";

/// Returns `document` as a `200` JSON answer no cache keeps.
pub fn answer<T: Serialize>(document: &T) -> Response {
    body(StatusCode::OK, document, JSON)
}

/// Returns the problem of `kind` with `detail`, at the status `kind` is
/// used with.
#[must_use]
pub fn refuse(kind: Kind, detail: &'static str) -> Response {
    problem::refuse(kind, detail)
}

/// Returns the answer of a route the store could not serve: `503` when the
/// store could not be read, `500` otherwise, the cause logged by its chain.
#[must_use]
pub fn failed(error: &OperatorError) -> Response {
    tracing::error!(error = %crate::chain(error), "an operator request could not be answered");
    match error {
        OperatorError::Store(_) => refuse(Kind::StoreUnavailable, "the store could not be read"),
        _ => problem::blank(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the request could not be answered",
        ),
    }
}

/// Returns the `503` of a request whose audit record could not be stored.
#[must_use]
pub fn unaudited(error: &Unrecorded) -> Response {
    tracing::error!(error = %crate::chain(error), "an operator answer was withheld: its audit record could not be stored");
    refuse(Kind::AuditUnavailable, UNAUDITED)
}

/// Reads the JSON document `T` a request posts with the bytes it was read
/// from, or the refusal: `415` for another media type, `413` above the body
/// ceiling and `400` for a body that is not the document.
///
/// # Errors
///
/// Returns the refusal's response.
pub async fn posted<T: DeserializeOwned>(
    parts: &Parts,
    body: Body,
) -> Result<(T, Vec<u8>), Box<Response>> {
    let essence = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if !essence.is_some_and(|essence| essence.eq_ignore_ascii_case(JSON)) {
        return Err(Box::new(problem::blank(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "the operator API reads a body as application/json",
        )));
    }
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return Err(Box::new(problem::blank(
            StatusCode::PAYLOAD_TOO_LARGE,
            "the request body is larger than this server reads",
        )));
    };
    // NOTE: no specification governs this: our own design; serde's message
    // can quote the value it stopped at, so a refusal says only that it failed.
    serde_json::from_slice(&bytes)
        .map(|document| (document, bytes.to_vec()))
        .map_err(|_unread| Box::new(refuse(Kind::InvalidRequest, NOT_THE_DOCUMENT)))
}

/// Reads the query of `parts` as `T`, or the `400` of a query that does
/// not read.
///
/// # Errors
///
/// Returns the refusal's response.
pub fn query<T: DeserializeOwned + Default>(parts: &Parts) -> Result<T, Box<Response>> {
    if parts.uri.query().is_none() {
        return Ok(T::default());
    }
    axum::extract::Query::try_from_uri(&parts.uri)
        .map(|axum::extract::Query(read)| read)
        .map_err(|_unread| Box::new(refuse(Kind::InvalidRequest, NOT_THE_QUERY)))
}

/// Returns the call of the request `parts` holds, at the operator API's
/// base.
#[must_use]
pub fn call(operator: &Operator, parts: &Parts) -> Call {
    let remote = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip());
    Call::of(
        &access(parts),
        remote,
        &operator.endpoint,
        crate::http::exchanged_request_id(parts),
    )
}

/// Returns the actor a steward action records: the subject of the caller's
/// token, or the Basic user-id.
#[must_use]
pub fn actor(parts: &Parts) -> String {
    access(parts).caller().map_or_else(
        || String::from("anonymous"),
        |caller| caller.subject().to_owned(),
    )
}

/// Returns the request id the request carries.
#[must_use]
pub fn request_id(parts: &Parts) -> Option<String> {
    parts
        .extensions
        .get::<RequestId>()
        .map(RequestId::id)
        .map(str::to_owned)
}

/// Returns the access the guard left on the request.
fn access(parts: &Parts) -> Access {
    parts
        .extensions
        .get::<Access>()
        .cloned()
        .unwrap_or(Access::Open)
}

/// Stores `exchanges`, the first in the request's reserved place.
///
/// # Errors
///
/// Returns [`Unrecorded`] when a record cannot be written or stored.
pub async fn record(
    operator: &Operator,
    parts: &Parts,
    exchanges: Vec<Exchange>,
) -> Result<(), Unrecorded> {
    let mut exchanges = exchanges.into_iter();
    if let Some(first) = exchanges.next() {
        match parts.extensions.get::<Pending>() {
            Some(pending) => pending.record(&operator.audit, &first).await?,
            None => operator.audit.record(vec![first]).await?,
        }
    }
    operator.audit.record(exchanges.collect()).await
}

/// Returns the uuid `text` names, or `None` for text that names none.
#[must_use]
pub fn uuid(text: &str) -> Option<uuid::Uuid> {
    // NOTE: no specification governs this: our own design; an id that is no
    // uuid names nothing the store holds, so it is answered as not found.
    uuid::Uuid::try_parse(text).ok()
}

/// Returns the uuid the `{id}` of the matched route names in the request's
/// path, or `None` when it names none.
#[must_use]
pub fn path_id(parts: &Parts) -> Option<uuid::Uuid> {
    let template = parts.extensions.get::<MatchedPath>()?.as_str();
    let position = template.split('/').position(|segment| segment == "{id}")?;
    let path = parts.uri.path();
    if path.split('/').count() != template.split('/').count() {
        return None;
    }
    path.split('/').nth(position).and_then(uuid)
}
