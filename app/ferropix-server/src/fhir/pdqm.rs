// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The PDQm routes of the FHIR face.
//!
//! ITI-78 by `GET {base}/fhir/Patient` and
//! `POST {base}/fhir/Patient/_search` (PDQm 3.2.0 §2:3.78.4.1.2), the
//! retrieve `GET {base}/fhir/Patient/{id}` (§2:3.78.4.3), and ITI-119 by
//! `POST {base}/fhir/Patient/$match` (§2:3.119.4.1.2), over
//! [`crate::pdqm::Supplier`].
//!
//! Each answer is FHIR JSON or XML as `_format` or `Accept` asks
//! (§2:3.78.4.1.2.4), a format neither names answered `406` for a search
//! and a match (§2:3.78.4.1.3, Case 5) and `400` for a retrieve
//! (§2:3.78.4.4.2). Every request, answered or refused, writes the PDQm
//! Supplier audit record with its raw request base64 (BALP 1.1.4
//! §1:52.4.1.4), one per person the answer names (§1:52.4.1.6), before the
//! answer is sent; a record that cannot be stored refuses the answer with
//! `503`. No log line, span or error carries a value of the request.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodFilter, MethodRouter, on};
use ferrohealth_smart::resource::Access;
use http::request::Parts;
use http::{HeaderValue, Method, StatusCode, header};
use ihe_atna::balp::{Exchange, Outcome};
use ihe_iti::outcome::IssueType;
use ihe_iti::pdqm::server::Refusal;
use ihe_iti::pdqm::server::matching::MatchRequest;
use ihe_iti::pdqm::server::search::PatientSearch;
use ihe_iti::server::answer::{Answer, Resource, Severity};
use ihe_iti::server::format::Format;
use secrecy::SecretString;
use uuid::Uuid;

use crate::audit::records::{self, Call, Search};
use crate::audit::reserve::{Pending, UNAUDITED};
use crate::audit::{Audit, Unrecorded};
use crate::auth::table::Route;
use crate::pdqm::{PAGE_PARAMETER, PdqError, Retrieved, Searched, Supplier};

/// The media type of a search posted as a form (FHIR R4 http.html, search).
const FORM: &str = "application/x-www-form-urlencoded";

/// What every PDQm route reads.
#[derive(Debug, Clone)]
pub struct Face {
    /// The Supplier.
    supplier: Arc<Supplier>,
    /// The audit trail.
    audit: Audit,
}

impl Face {
    /// Returns the routes' state over `supplier`, recording to `audit`.
    #[must_use]
    pub fn new(supplier: Arc<Supplier>, audit: Audit) -> Self {
        Self { supplier, audit }
    }
}

/// Returns the handler of `route` when it is a PDQm route, under `filter`.
#[must_use]
pub fn handler(route: &Route, filter: MethodFilter, face: &Arc<Face>) -> Option<MethodRouter> {
    let face = Arc::clone(face);
    let handler = match (route.method.clone(), route.path) {
        (Method::GET, "/fhir/Patient") => on(filter, move |request: Request| {
            search(Arc::clone(&face), request, false)
        }),
        (Method::POST, "/fhir/Patient/_search") => on(filter, move |request: Request| {
            search(Arc::clone(&face), request, true)
        }),
        (Method::GET, "/fhir/Patient/{id}") => on(filter, move |request: Request| {
            retrieve(Arc::clone(&face), request)
        }),
        (Method::POST, "/fhir/Patient/$match") => on(filter, move |request: Request| {
            matches(Arc::clone(&face), request)
        }),
        _ => return None,
    };
    Some(handler)
}

/// ITI-78, the search by `GET` or by `POST` of a form.
async fn search(face: Arc<Face>, request: Request, post: bool) -> Response {
    let (parts, body) = request.into_parts();
    let query = parts.uri.query().unwrap_or_default().to_owned();
    let call = call(&face, &parts);
    let (rest, posted) = if post {
        ("Patient/_search", true)
    } else {
        ("Patient", false)
    };
    let mut raw_body = None;
    let parsed = if posted {
        if !media_type_is(&parts, FORM) {
            let answer = Answer::outcome(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Severity::Error,
                IssueType::NotSupported,
                "a search posts its parameters as application/x-www-form-urlencoded",
            );
            let raw = raw(&face, rest, &query, None);
            return refuse(&face, &parts, &call, Search::Query, raw, &answer, &query).await;
        }
        let Some(bytes) = read(body).await else {
            return too_large();
        };
        raw_body = Some(bytes.clone());
        // NOTE: FHIR R4 http.html, search: the parameters of a POST may sit in
        // the URL and the body; the query joins the form, a `+` kept literal.
        let mut form = bytes;
        if !query.is_empty() {
            form.push(b'&');
            form.extend(query.replace('+', "%2B").into_bytes());
        }
        let cursor = cursor_of(&form);
        PatientSearch::from_form(&form).map(|search| (search, cursor))
    } else {
        let cursor = cursor_of(query.as_bytes());
        PatientSearch::from_query(&query).map(|search| (search, cursor))
    };
    let raw = raw(&face, rest, &query, raw_body.as_deref().map(|b| (FORM, b)));
    let (search, cursor) = match parsed {
        Ok(parsed) => parsed,
        Err(error) => {
            return refuse(
                &face,
                &parts,
                &call,
                Search::Query,
                raw,
                &error.refusal(),
                &query,
            )
            .await;
        }
    };
    let format = match Format::negotiate(search.format(), accept(&parts)) {
        Ok(format) => format,
        Err(unacceptable) => {
            return refuse(
                &face,
                &parts,
                &call,
                Search::Query,
                raw,
                &unacceptable.refusal(),
                &query,
            )
            .await;
        }
    };
    let after = match cursor {
        Cursor::None => None,
        Cursor::After(after) => Some(after),
        Cursor::Malformed => {
            let answer = Answer::outcome(
                StatusCode::BAD_REQUEST,
                Severity::Error,
                IssueType::Value,
                "the page parameter names no page of this server",
            );
            return refuse(&face, &parts, &call, Search::Query, raw, &answer, &query).await;
        }
    };
    match face.supplier.search(&search, after).await {
        Ok(Searched::Found { answer, persons }) => {
            let exchanges = records::pdq_search(
                &call,
                Search::Query,
                &raw,
                &references(&persons),
                Outcome::Success,
            );
            send(&face, &parts, exchanges, answer.as_ref(), format).await
        }
        Ok(Searched::Refused(refusal)) => {
            let exchanges =
                records::pdq_search(&call, Search::Query, &raw, &[], Outcome::MinorFailure);
            send(&face, &parts, exchanges, &refusal.answer(), format).await
        }
        Err(error) => failed(&face, &parts, &call, Search::Query, &raw, &error, format).await,
    }
}

/// ITI-78, the retrieve of one Patient by its id (PDQm 3.2.0 §2:3.78.4.3).
async fn retrieve(face: Arc<Face>, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let query = parts.uri.query().unwrap_or_default().to_owned();
    let id = parts
        .uri
        .path()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned();
    let call = call(&face, &parts);
    let raw = raw(&face, &format!("Patient/{id}"), &query, None);
    let Ok(format) = Format::negotiate(raw_format(&query).as_deref(), accept(&parts)) else {
        // NOTE: PDQm 3.2.0 §2:3.78.4.4.2, a retrieve the Supplier cannot write in
        // the requested format "SHALL respond with an HTTP 400 error".
        let answer = Answer::outcome(
            StatusCode::BAD_REQUEST,
            Severity::Error,
            IssueType::NotSupported,
            "the requested response format is not supported",
        );
        return refuse(&face, &parts, &call, Search::Query, raw, &answer, &query).await;
    };
    match face.supplier.retrieve(&id).await {
        Ok(Retrieved::Found { answer, person }) => {
            let exchanges = records::pdq_search(
                &call,
                Search::Query,
                &raw,
                &references(&[person]),
                Outcome::Success,
            );
            send(&face, &parts, exchanges, answer.as_ref(), format).await
        }
        Ok(Retrieved::NotFound) => {
            let exchanges =
                records::pdq_search(&call, Search::Query, &raw, &[], Outcome::MinorFailure);
            send(
                &face,
                &parts,
                exchanges,
                &Refusal::PatientNotFound.answer(),
                format,
            )
            .await
        }
        Err(error) => failed(&face, &parts, &call, Search::Query, &raw, &error, format).await,
    }
}

/// ITI-119, `$match` (PDQm 3.2.0 §2:3.119.4.1.2).
async fn matches(face: Arc<Face>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let query = parts.uri.query().unwrap_or_default().to_owned();
    let call = call(&face, &parts);
    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let Some(bytes) = read(body).await else {
        return too_large();
    };
    let media = content_type
        .as_deref()
        .unwrap_or("application/octet-stream");
    let raw = raw(&face, "Patient/$match", &query, Some((media, &bytes)));
    let Some(body_format) = Format::of_content_type(content_type.as_deref()) else {
        let answer = Format::unsupported_media_type();
        return refuse(&face, &parts, &call, Search::Match, raw, &answer, &query).await;
    };
    let format = match Format::negotiate(raw_format(&query).as_deref(), accept(&parts)) {
        Ok(format) => format,
        Err(unacceptable) => {
            return refuse(
                &face,
                &parts,
                &call,
                Search::Match,
                raw,
                &unacceptable.refusal(),
                &query,
            )
            .await;
        }
    };
    let request = match MatchRequest::from_request(&query, body_format, &bytes) {
        Ok(request) => request,
        Err(error) => {
            let exchanges =
                records::pdq_search(&call, Search::Match, &raw, &[], Outcome::MinorFailure);
            return send(&face, &parts, exchanges, &error.refusal(), format).await;
        }
    };
    match face.supplier.matches(&request).await {
        Ok(found) => {
            let exchanges = records::pdq_search(
                &call,
                Search::Match,
                &raw,
                &references(&found.persons),
                Outcome::Success,
            );
            send(&face, &parts, exchanges, &found.answer, format).await
        }
        Err(error) => failed(&face, &parts, &call, Search::Match, &raw, &error, format).await,
    }
}

/// Records `exchanges` and answers `answer` in `format`, or `503` when a
/// record cannot be stored.
async fn send<T: Resource>(
    face: &Face,
    parts: &Parts,
    exchanges: Vec<Exchange>,
    answer: &Answer<T>,
    format: Format,
) -> Response {
    if let Err(error) = record(face, parts, exchanges).await {
        tracing::error!(error = %crate::chain(&error), "a PDQm answer was withheld: its audit record could not be stored");
        return written(
            &Answer::outcome(
                StatusCode::SERVICE_UNAVAILABLE,
                Severity::Error,
                IssueType::Transient,
                UNAUDITED,
            ),
            format,
        );
    }
    written(answer, format)
}

/// Answers a request refused before it ran, in the format it asks for when
/// it names one the server writes, else JSON.
async fn refuse<T: Resource>(
    face: &Face,
    parts: &Parts,
    call: &Call,
    search: Search,
    raw: SecretString,
    answer: &Answer<T>,
    query: &str,
) -> Response {
    let format =
        Format::negotiate(raw_format(query).as_deref(), accept(parts)).unwrap_or(Format::Json);
    let exchanges = records::pdq_search(call, search, &raw, &[], Outcome::MinorFailure);
    send(face, parts, exchanges, answer, format).await
}

/// Answers a request the store could not serve: `503` when the store could
/// not be read, `500` otherwise.
async fn failed(
    face: &Face,
    parts: &Parts,
    call: &Call,
    search: Search,
    raw: &SecretString,
    error: &PdqError,
    format: Format,
) -> Response {
    tracing::error!(error = %crate::chain(error), "a PDQm request could not be answered");
    let answer = match error {
        PdqError::Store(_) => Answer::outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            Severity::Error,
            IssueType::Transient,
            "the store could not be read",
        ),
        _ => Answer::outcome(
            StatusCode::INTERNAL_SERVER_ERROR,
            Severity::Error,
            IssueType::Exception,
            "the request could not be answered",
        ),
    };
    let exchanges = records::pdq_search(call, search, raw, &[], Outcome::SeriousFailure);
    send(face, parts, exchanges, &answer, format).await
}

/// Stores `exchanges`, the first in the request's reserved place.
async fn record(face: &Face, parts: &Parts, exchanges: Vec<Exchange>) -> Result<(), Unrecorded> {
    let mut exchanges = exchanges.into_iter();
    if let Some(first) = exchanges.next() {
        match parts.extensions.get::<Pending>() {
            Some(pending) => pending.record(&face.audit, &first).await?,
            None => face.audit.record(vec![first]).await?,
        }
    }
    face.audit.record(exchanges.collect()).await
}

/// Returns `answer` encoded in `format`, never kept by a cache, or a `500`
/// when it cannot be encoded.
fn written<T: Resource>(answer: &Answer<T>, format: Format) -> Response {
    let Ok(bytes) = answer.encode(format) else {
        tracing::error!("a PDQm answer could not be encoded");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = (answer.status(), Body::from(bytes)).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.media_type()),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Returns the call of the request `parts` holds.
fn call(face: &Face, parts: &Parts) -> Call {
    let access = parts
        .extensions
        .get::<Access>()
        .cloned()
        .unwrap_or(Access::Open);
    let remote = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip());
    Call::of(
        &access,
        remote,
        face.supplier.fhir_base(),
        crate::http::exchanged_request_id(parts),
    )
}

/// Returns the raw request BALP records: the URL of `rest` under the FHIR
/// base with `query`, and the media type and body of a `POST`.
fn raw(face: &Face, rest: &str, query: &str, body: Option<(&str, &[u8])>) -> SecretString {
    let mut url = face.supplier.url(rest);
    if !query.is_empty() {
        url.set_query(Some(query));
    }
    match body {
        None => records::raw_request(&url, None),
        Some((media_type, bytes)) => {
            let text = SecretString::from(String::from_utf8_lossy(bytes).into_owned());
            records::raw_request(&url, Some((media_type, &text)))
        }
    }
}

/// Returns the `Patient/{id}` reference of each person.
fn references(persons: &[Uuid]) -> Vec<String> {
    persons
        .iter()
        .map(|person| format!("Patient/{person}"))
        .collect()
}

/// Returns the request's `Accept` header.
fn accept(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
}

/// Returns whether the request's `Content-Type` is `media_type`, its
/// parameters aside.
fn media_type_is(parts: &Parts, media_type: &str) -> bool {
    parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case(media_type))
}

/// Returns the first `_format` the query names.
fn raw_format(query: &str) -> Option<String> {
    url::form_urlencoded::parse(query.replace('+', "%2B").as_bytes())
        .find(|(name, _)| name == "_format")
        .map(|(_, value)| value.into_owned())
}

/// The page a search asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cursor {
    /// The first page.
    None,
    /// The page after this person.
    After(Uuid),
    /// A page parameter that names no page.
    Malformed,
}

/// Returns the page the parameters `pairs` ask for.
fn cursor_of(pairs: &[u8]) -> Cursor {
    let mut values = url::form_urlencoded::parse(pairs)
        .filter(|(name, _)| name == PAGE_PARAMETER)
        .map(|(_, value)| value.into_owned());
    match (values.next(), values.next()) {
        (None, _) => Cursor::None,
        (Some(value), None) => Uuid::try_parse(&value).map_or(Cursor::Malformed, Cursor::After),
        (Some(_), Some(_)) => Cursor::Malformed,
    }
}

/// Returns the `413` the server's body ceiling answers.
fn too_large() -> Response {
    written(
        &Answer::outcome(
            StatusCode::PAYLOAD_TOO_LARGE,
            Severity::Error,
            IssueType::TooLong,
            "the request body is larger than this server reads",
        ),
        Format::Json,
    )
}

/// Reads a request body under the server's body ceiling, `None` when it is
/// above it.
async fn read(body: Body) -> Option<Vec<u8>> {
    axum::body::to_bytes(body, usize::MAX)
        .await
        .ok()
        .map(|bytes| bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{Cursor, cursor_of, raw_format};

    #[test]
    fn the_page_parameter_names_one_person_or_is_refused() {
        let id = Uuid::nil();
        assert_eq!(Cursor::None, cursor_of(b"family=x"));
        assert_eq!(
            Cursor::After(id),
            cursor_of(format!("family=x&_page={id}").as_bytes())
        );
        assert_eq!(Cursor::Malformed, cursor_of(b"_page=x"));
        assert_eq!(
            Cursor::Malformed,
            cursor_of(format!("_page={id}&_page={id}").as_bytes())
        );
    }

    #[test]
    fn the_format_parameter_is_read_from_the_query() {
        assert_eq!(Some(String::from("xml")), raw_format("a=b&_format=xml"));
        assert_eq!(
            Some(String::from("application/fhir+xml")),
            raw_format("_format=application/fhir+xml")
        );
        assert_eq!(None, raw_format("family=x"));
    }
}
