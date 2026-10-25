// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FHIR encodings a server reads and writes: JSON and XML (FHIR R4
//! http.html, "Content Types and encodings",
//! <http://hl7.org/fhir/R4/http.html#mime-type>).

use fhir_types::codec::{DecodeErrorKind, Object, Value};
use fhir_types::r4::operation_outcome::OperationOutcome;
use http::StatusCode;

use super::answer::{Answer, Severity};
use crate::outcome::IssueType;

/// A FHIR encoding: the media type of a request body or of an answer.
///
/// # Examples
///
/// ```
/// use ihe_iti::server::format::Format;
///
/// let body = Format::of_content_type(Some("application/fhir+xml; charset=utf-8"));
/// assert_eq!(body, Some(Format::Xml));
/// let answer = Format::negotiate(Some("json"), Some("application/fhir+xml"));
/// assert_eq!(answer, Ok(Format::Json));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Format {
    /// FHIR JSON, `application/fhir+json`.
    #[default]
    Json,
    /// FHIR XML, `application/fhir+xml`.
    Xml,
}

impl Format {
    /// Returns the format a request body's `Content-Type` names, or `None`
    /// for a missing or another media type, which a server answers with
    /// [`Format::unsupported_media_type`].
    ///
    /// The FHIR media types, and the plain `application/json`,
    /// `application/xml` and `text/xml` FHIR R4 also accepts, are recognised
    /// with any parameters.
    #[must_use]
    pub fn of_content_type(content_type: Option<&str>) -> Option<Self> {
        content_type.and_then(media)
    }

    /// Returns the format of the answer: the `_format` parameter when there is
    /// one, which FHIR R4 lets override `Accept`, else the most preferred
    /// media range of `accept` that names JSON or XML, else JSON.
    ///
    /// `_format` takes the short forms `json` and `xml` and every media type
    /// [`Format::of_content_type`] takes. A wildcard range (`*/*`,
    /// `application/*`) is JSON, and a range with `q=0` is refused.
    ///
    /// # Errors
    /// [`Unacceptable`] when `_format` names no FHIR encoding, or `accept`
    /// is present and admits neither, which PDQm answers with a `406`
    /// (§2:3.78.4.1.3, Case 5).
    pub fn negotiate(format: Option<&str>, accept: Option<&str>) -> Result<Self, Unacceptable> {
        if let Some(format) = format {
            return match format.trim() {
                "json" => Ok(Self::Json),
                "xml" => Ok(Self::Xml),
                other => media(other).ok_or(Unacceptable),
            };
        }
        let Some(accept) = accept.filter(|accept| !accept.trim().is_empty()) else {
            return Ok(Self::Json);
        };
        let mut best: Option<(u16, Self)> = None;
        for range in accept.split(',') {
            let mut parts = range.split(';');
            let essence = parts.next().unwrap_or_default().trim();
            let quality = parts
                .filter_map(|parameter| parameter.trim().strip_prefix("q="))
                .find_map(quality)
                .unwrap_or(1000);
            let format = match essence {
                "*/*" | "application/*" => Some(Self::Json),
                other => media(other),
            };
            if let Some(format) = format
                && quality > 0
                && best.is_none_or(|(held, _)| quality > held)
            {
                best = Some((quality, format));
            }
        }
        best.map(|(_, format)| format).ok_or(Unacceptable)
    }

    /// Returns the media type an answer in this format carries.
    #[must_use]
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Json => "application/fhir+json",
            Self::Xml => "application/fhir+xml",
        }
    }

    /// Returns the refusal of a request body whose media type is no FHIR
    /// encoding: a `415` with a `not-supported` issue (FHIR R4 http.html,
    /// "Content Types and encodings").
    #[must_use]
    pub fn unsupported_media_type() -> Answer<OperationOutcome> {
        Answer::outcome(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Severity::Error,
            IssueType::NotSupported,
            "the request body is neither FHIR JSON nor FHIR XML",
        )
    }

    /// The resource `body` holds, in the JSON object model.
    pub(crate) fn decode(self, body: &[u8]) -> Result<Object, BodyError> {
        let object = match self {
            Self::Json => {
                let value: Value =
                    serde_json::from_slice(body).map_err(|error| BodyError::NotJson {
                        line: error.line(),
                        column: error.column(),
                    })?;
                let Value::Object(object) = value else {
                    return Err(BodyError::NotAResource);
                };
                // NOTE: PIXm 3.1.0 §2:3.104.4.1.2, the server writes FHIR XML too, so a JSON
                // body with no XML form, or one that does not read back as itself, is refused.
                let schemas = &fhir_types::r4::schema::SCHEMAS;
                let round_trips = fhir_types::xml::to_xml(schemas, &object)
                    .ok()
                    .and_then(|xml| fhir_types::xml::from_xml(schemas, &xml).ok())
                    .is_some_and(|read| read == object);
                if !round_trips {
                    return Err(BodyError::NoXmlForm);
                }
                object
            }
            Self::Xml => {
                let text = std::str::from_utf8(body).map_err(|_not_utf8| BodyError::NotXml)?;
                fhir_types::xml::from_xml(&fhir_types::r4::schema::SCHEMAS, text).map_err(
                    |error| match error.kind {
                        DecodeErrorKind::MalformedXml { .. } => BodyError::NotXml,
                        kind => BodyError::Decode { kind },
                    },
                )?
            }
        };
        if object.get("resourceType").and_then(Value::as_str).is_none() {
            return Err(BodyError::NotAResource);
        }
        Ok(object)
    }
}

/// The `q` of a media range, in thousandths (RFC 9110 §12.4.2), or `None`
/// for a malformed one.
fn quality(text: &str) -> Option<u16> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let digits = format!("{fraction:0<3}");
    match whole {
        "0" => digits.parse().ok(),
        "1" if digits == "000" => Some(1000),
        _ => None,
    }
}

/// The format the media type `text` names, parameters aside.
fn media(text: &str) -> Option<Format> {
    let essence = text.split(';').next().unwrap_or_default().trim();
    let is = |name: &str| essence.eq_ignore_ascii_case(name);
    if is("application/fhir+json") || is("application/json") {
        Some(Format::Json)
    } else if is("application/fhir+xml") || is("application/xml") || is("text/xml") {
        Some(Format::Xml)
    } else {
        None
    }
}

/// The answer cannot be written in any format the request accepts: PDQm's
/// Case 5 (§2:3.78.4.1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the request accepts no FHIR encoding the server writes")]
pub struct Unacceptable;

impl Unacceptable {
    /// Returns the refusal: a `406` with an `error` issue of type
    /// `not-supported` (PDQm 3.2.0 §2:3.78.4.1.3, Case 5).
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "every reader's error answers with its refusal by this one method"
    )]
    pub fn refusal(self) -> Answer<OperationOutcome> {
        Answer::outcome(
            StatusCode::NOT_ACCEPTABLE,
            Severity::Error,
            IssueType::NotSupported,
            "the requested response format is not supported",
        )
    }
}

/// Why a request body holds no FHIR R4 resource.
///
/// No variant carries text from the body: a body holds patient data, and a
/// parser's reason may quote it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BodyError {
    /// The body is not JSON.
    #[error("the body is not JSON (line {line}, column {column})")]
    NotJson {
        /// The line of the syntax error.
        line: usize,
        /// The column of the syntax error.
        column: usize,
    },
    /// The body is not well-formed XML in UTF-8.
    #[error("the body is not well-formed XML")]
    NotXml,
    /// The body is not a resource with a `resourceType`.
    #[error("the body is not a FHIR resource")]
    NotAResource,
    /// The body is FHIR JSON whose FHIR XML form does not read back, such as
    /// one with a narrative `div` that is no XHTML, so the server could not
    /// answer with it in XML.
    #[error("the body has no FHIR XML form")]
    NoXmlForm,
    /// The body does not decode as FHIR R4.
    #[error("the body does not decode as FHIR R4: {kind}")]
    Decode {
        /// Why it was refused.
        kind: DecodeErrorKind,
    },
}

impl BodyError {
    pub(crate) fn decode(error: fhir_types::codec::DecodeError) -> Self {
        match error.kind {
            DecodeErrorKind::MalformedXml { .. } => Self::NotXml,
            kind => Self::Decode { kind },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BodyError, Format, Unacceptable};

    #[test]
    fn the_content_type_names_the_body_format() {
        for (given, want) in [
            (Some("application/fhir+json"), Some(Format::Json)),
            (Some("application/json; charset=utf-8"), Some(Format::Json)),
            (Some("Application/FHIR+XML"), Some(Format::Xml)),
            (Some("text/xml"), Some(Format::Xml)),
            (Some("text/plain"), None),
            (None, None),
        ] {
            assert_eq!(Format::of_content_type(given), want, "{given:?}");
        }
    }

    #[test]
    fn format_overrides_accept_and_accept_is_read_by_quality() {
        assert_eq!(
            Format::negotiate(Some("application/fhir+xml"), Some("application/fhir+json")),
            Ok(Format::Xml),
            "_format wins"
        );
        assert_eq!(
            Format::negotiate(None, None),
            Ok(Format::Json),
            "no preference"
        );
        assert_eq!(
            Format::negotiate(
                None,
                Some("application/fhir+json;q=0.5, application/fhir+xml;q=0.9")
            ),
            Ok(Format::Xml),
            "the higher quality"
        );
        assert_eq!(
            Format::negotiate(None, Some("text/html, */*;q=0.8")),
            Ok(Format::Json),
            "a wildcard is JSON"
        );
        assert_eq!(
            Format::negotiate(None, Some("text/html")),
            Err(Unacceptable),
            "no FHIR encoding"
        );
        assert_eq!(
            Format::negotiate(None, Some("application/fhir+xml;q=0")),
            Err(Unacceptable),
            "q=0 refuses"
        );
        assert_eq!(
            Format::negotiate(Some("ttl"), None),
            Err(Unacceptable),
            "Turtle is not written"
        );
    }

    #[test]
    fn both_encodings_decode_to_the_same_object() {
        let json = br#"{"resourceType":"Patient","active":true}"#;
        let xml = br#"<Patient xmlns="http://hl7.org/fhir"><active value="true"/></Patient>"#;
        assert_eq!(
            Format::Json.decode(json),
            Format::Xml.decode(xml),
            "one object model"
        );
        assert_eq!(
            Format::Json.decode(b"[1]"),
            Err(BodyError::NotAResource),
            "an array"
        );
        assert_eq!(
            Format::Xml.decode(b"<Patient"),
            Err(BodyError::NotXml),
            "truncated XML"
        );
    }

    #[test]
    fn a_json_body_with_no_xml_form_is_refused() {
        let narrative = br#"{"resourceType":"Patient","text":{"status":"generated","div":"<div xmlns=\"http://www.w3.org/1999/xhtml\"><p>open"}}"#;
        assert_eq!(
            Format::Json.decode(narrative),
            Err(BodyError::NoXmlForm),
            "a narrative that is no XHTML"
        );
        let narrative = br#"{"resourceType":"Patient","text":{"status":"generated","div":"<div xmlns=\"http://www.w3.org/1999/xhtml\"><p>closed</p></div>"}}"#;
        assert!(Format::Json.decode(narrative).is_ok(), "an XHTML narrative");
        let narrative = br#"{"resourceType":"Patient","text":{"status":"generated","div":"<div xmlns=\"http://www.w3.org/1999/xhtml\"><p class=\"a\"%title=\"b\">x</p></div>"}}"#;
        assert!(
            matches!(Format::Json.decode(narrative), Err(BodyError::NoXmlForm)),
            "a narrative whose XML form reads back changed"
        );
    }
}
