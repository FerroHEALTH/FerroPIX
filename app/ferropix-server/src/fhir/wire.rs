// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The encodings of the FHIR face's answers: JSON and XML on every served
//! route.
//!
//! `_format` is honoured over `Accept`, and a request that accepts neither
//! answers `406` (FHIR R4 http.html, "Content Types and encodings",
//! §3.1.0.1.10; PDQm 3.2.0 §2:3.78.4.1.3, Case 5). The negotiation itself is `ihe-iti`'s ([`Format::negotiate`]); this
//! module takes it apart from the HTTP request and puts the encoded answer
//! on an `axum` response that no cache keeps.

use axum::body::Body;
use axum::response::{IntoResponse, Response};
use fhir_types::codec::Json;
use http::{HeaderMap, HeaderValue, StatusCode, header};
use ihe_iti::server::answer::{Answer, Resource};
use ihe_iti::server::format::Format;

use crate::fhir::status::Case;

/// Returns the first `_format` of the query `query`, the value FHIR R4
/// lets override `Accept` (http.html §3.1.0.1.10).
///
/// Only that parameter is read; every other one is the transaction
/// reader's.
#[must_use]
pub fn format_parameter(query: Option<&str>) -> Option<String> {
    url::form_urlencoded::parse(query?.as_bytes())
        .find(|(name, _)| name == "_format")
        .map(|(_, value)| value.into_owned())
}

/// Returns the format of the answer to a request with `query` and
/// `headers`, or the `406` to send in JSON.
///
/// # Errors
/// Returns the `406` response when the request accepts no FHIR encoding.
pub fn negotiate(query: Option<&str>, headers: &HeaderMap) -> Result<Format, Box<Response>> {
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok());
    Format::negotiate(format_parameter(query).as_deref(), accept)
        .map_err(|_unacceptable| Box::new(send(&Case::NotAcceptable.answer(), Format::Json)))
}

/// Returns the format a request body's `Content-Type` in `headers` names.
///
/// # Errors
/// Returns [`Case::UnsupportedMediaType`] for a body that is neither FHIR
/// JSON nor FHIR XML.
pub fn body_format(headers: &HeaderMap) -> Result<Format, Case> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    Format::of_content_type(content_type).ok_or(Case::UnsupportedMediaType)
}

/// Returns `answer` encoded in `format` with its status.
pub fn send<T: Resource>(answer: &Answer<T>, format: Format) -> Response {
    match answer.encode(format) {
        Ok(bytes) => respond(answer.status(), bytes, format),
        Err(error) => {
            tracing::error!(%error, "an answer of the FHIR face could not be written");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Returns `resource`, a resource `ihe-iti` does not answer with, as a
/// `200` body in `format`.
pub fn send_resource<T: Json + serde::Serialize>(resource: &T, format: Format) -> Response {
    if let Some(bytes) = encoded(resource, format) {
        return respond(StatusCode::OK, bytes, format);
    }
    tracing::error!("a resource of the FHIR face could not be written");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// Returns `resource` written in `format`, `None` when it has no form there.
pub fn encoded<T: Json + serde::Serialize>(resource: &T, format: Format) -> Option<Vec<u8>> {
    match format {
        Format::Xml => resource.to_json().ok().and_then(|object| {
            fhir_types::xml::to_xml(&fhir_types::r4::schema::SCHEMAS, &object)
                .ok()
                .map(String::into_bytes)
        }),
        _ => serde_json::to_vec(resource).ok(),
    }
}

/// The response of `status` carrying `bytes` of `format`.
fn respond(status: StatusCode, bytes: Vec<u8>, format: Format) -> Response {
    let mut response = (status, Body::from(bytes)).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.media_type()),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use http::{HeaderMap, HeaderValue, StatusCode, header};
    use ihe_iti::server::format::Format;

    use super::{body_format, format_parameter, negotiate};
    use crate::fhir::status::Case;

    #[test]
    fn format_is_read_from_the_query_and_overrides_accept() {
        assert_eq!(
            Some(String::from("xml")),
            format_parameter(Some("sourceIdentifier=a%7Cb&_format=xml"))
        );
        assert_eq!(None, format_parameter(Some("sourceIdentifier=a%7Cb")));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("application/fhir+json"),
        );
        assert_eq!(
            Some(Format::Xml),
            negotiate(Some("_format=xml"), &headers).ok()
        );
        assert_eq!(Some(Format::Json), negotiate(None, &headers).ok());
        let refused = negotiate(Some("_format=text/turtle"), &headers).expect_err("406");
        assert_eq!(StatusCode::NOT_ACCEPTABLE, refused.status());
    }

    #[test]
    fn a_body_that_is_no_fhir_encoding_answers_415() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        assert_eq!(Err(Case::UnsupportedMediaType), body_format(&headers));
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/fhir+xml"),
        );
        assert_eq!(Ok(Format::Xml), body_format(&headers));
    }
}
