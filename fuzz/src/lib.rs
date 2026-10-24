// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What the fuzz targets share: the split of one input into its parts, the
//! format a body is read in, the check that a refusal quotes nothing from
//! the request, and the FHIR writers a round trip goes through.

use std::error::Error;
use std::fmt::Debug;

use fhir_types::codec::{Json, Value};
use fhir_types::r4::operation_outcome::OperationOutcome;
use fhir_types::r4::parameters::{Parameters, ParametersParameter, ParametersParameterValue};
use ihe_iti::pixm::server::query::PixQuery;
use ihe_iti::server::answer::Answer;
use ihe_iti::server::format::Format;
use secrecy::ExposeSecret;

/// The input up to its first NUL byte, and the bytes after it.
#[must_use]
pub fn split_at_nul(data: &[u8]) -> (&[u8], &[u8]) {
    match data.iter().position(|&byte| byte == 0) {
        Some(at) => (&data[..at], &data[at + 1..]),
        None => (data, &[]),
    }
}

/// The format of a body: XML when its first byte past any whitespace is
/// `<`, JSON otherwise, so one corpus reaches both decoders.
#[must_use]
pub fn sniff(body: &[u8]) -> Format {
    match body.iter().find(|byte| !byte.is_ascii_whitespace()) {
        Some(b'<') => Format::Xml,
        _ => Format::Json,
    }
}

/// Whether `text` holds only the characters the crate's own error and
/// refusal texts are written in.
///
/// Every error and every refusal of a reader is static text with at most a
/// number or a variant name in it (the crate's error types carry no value
/// from the request). A reader that quoted the request would also quote the
/// arbitrary bytes the fuzzer sends it, which fall outside this alphabet
/// (no specification governs this: our own design).
fn own_words(text: &str) -> bool {
    text.chars().all(|character| {
        character.is_ascii_alphanumeric()
            || matches!(
                character,
                ' ' | '$' | '\'' | '(' | ')' | '+' | ',' | '-' | '/' | ':' | '_' | '{' | '}' | '"'
            )
    })
}

/// Panics when the error, any error in its source chain, its `Debug`, or the
/// encoded refusal holds text from outside the crate's own words.
pub fn assert_quotes_nothing<E: Error + Debug>(error: &E, refusal: &Answer<OperationOutcome>) {
    let mut cause: Option<&dyn Error> = Some(error);
    while let Some(current) = cause {
        let text = current.to_string();
        assert!(own_words(&text), "an error quotes the request: {text:?}");
        cause = current.source();
    }
    let debug = format!("{error:?}");
    assert!(
        own_words(&debug),
        "an error's Debug quotes the request: {debug:?}"
    );
    for format in [Format::Json, Format::Xml] {
        let Ok(bytes) = refusal.encode(format) else {
            panic!("a refusal does not encode as {format:?}");
        };
        let Ok(text) = String::from_utf8(bytes) else {
            panic!("a refusal encodes to bytes that are not UTF-8");
        };
        let quoted = text
            .chars()
            .any(|character| !character.is_ascii() || matches!(character, '|' | '\\' | '%' | '@'));
        assert!(!quoted, "a refusal quotes the request: {text:?}");
    }
}

/// The resource written by the `fhir-types` writers in `format`, the bytes a
/// reader is handed back in a round trip.
///
/// # Panics
/// When a resource that was read cannot be written: a reader must never
/// accept what the writer of the same model refuses.
#[must_use]
pub fn write<T: Json>(resource: &T, format: Format) -> Vec<u8> {
    let Ok(object) = resource.to_json() else {
        panic!("a resource that was read does not write as JSON");
    };
    match format {
        Format::Xml => {
            let Ok(text) = fhir_types::xml::to_xml(&fhir_types::r4::schema::SCHEMAS, &object)
            else {
                panic!("a resource that was read does not write as XML");
            };
            text.into_bytes()
        }
        _ => {
            let Ok(bytes) = serde_json::to_vec(&Value::Object(object)) else {
                panic!("a resource that was read does not serialize as JSON");
            };
            bytes
        }
    }
}

/// `text` with the escapes FHIR R4 search gives `\`, `|`, `,` and `$`
/// (FHIR R4 search, "Escaping Search Parameters").
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '\\' | '|' | ',' | '$') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// Posts `read` back as the `Parameters` of the Query Parameters In profile,
/// in both encodings, and asserts each reads as the same request.
///
/// # Panics
/// When the request reads differently, or does not read, the second time.
pub fn pix_round_trip(read: &PixQuery) {
    let string = |name: &str, value: &str| ParametersParameter {
        name: name.into(),
        value: Some(ParametersParameterValue::String(value.into())),
        ..ParametersParameter::default()
    };
    let source = format!(
        "{}|{}",
        escape(read.source().system()),
        escape(read.source().value().expose_secret())
    );
    let mut parameter = vec![string("sourceIdentifier", &source)];
    parameter.extend(
        read.targets()
            .iter()
            .map(|target| string("targetSystem", &escape(target.as_str()))),
    );
    parameter.extend(read.format().map(|format| string("_format", format)));
    let parameters = Parameters {
        parameter,
        ..Parameters::default()
    };
    for format in [Format::Json, Format::Xml] {
        let Ok(again) = PixQuery::from_body(format, &write(&parameters, format)) else {
            panic!("a query that reads does not read back as {format:?} Parameters");
        };
        assert_eq!(again.source().system(), read.source().system());
        assert_eq!(
            again.source().value().expose_secret(),
            read.source().value().expose_secret(),
            "the source identifier changes across the Parameters"
        );
        let systems = |query: &PixQuery| {
            query
                .targets()
                .iter()
                .map(|target| target.as_str().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(systems(&again), systems(read));
        assert_eq!(again.format(), read.format());
    }
}
