// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The PIXm 3.1.0 corpus: each example of the vendored package but the audit
//! record, read by the ITI-83 client or the Manager side and, where the
//! Manager writes it, written back.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the vendored examples are read as JSON values for what they carry"
)]

use fhir_types::codec::Object;
use http::StatusCode;
use ihe_atna::user::OnBehalfOf;
use ihe_iti::pixm::identifier::{CrossReference, PatientReference, TargetIdentifier, TargetSystem};
use ihe_iti::pixm::server::Refusal;
use ihe_iti::pixm::server::answer::cross_references;
use ihe_iti::pixm::server::feed::Feed;
use ihe_iti::pixm::server::query::PixQuery;
use ihe_iti::server::format::Format;
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use super::{example, examples, is_audit, record, verdict};
use crate::pixm::{BLUE, FHIR_JSON, PROMPT, RED, RED_VALUE, client, manager, red_source, target};

/// The vendored package under `docs/specs/`.
const PACKAGE: &str = "ihe-pixm";

#[tokio::test]
async fn conformance_the_pixm_examples_hold_their_pass_list() {
    let mut cases = Vec::new();
    for id in examples(PACKAGE).into_iter().filter(|id| !is_audit(id)) {
        cases.push(verdict(id.clone(), check(id)).await);
    }
    record(
        "pixm",
        "The PIXm 3.1.0 examples of docs/specs/ihe-pixm, read and written through ihe-iti",
        cases,
    );
}

/// Checks the example `id` by the resource it holds.
async fn check(id: String) {
    let name = id.trim_start_matches("example/");
    if name.starts_with("Parameters-pixm-request-") {
        request(&id);
    } else if name.starts_with("Parameters-pixm-response-") {
        response(&id).await;
    } else if name.starts_with("OperationOutcome-") {
        not_found(&id).await;
    } else if name.starts_with("Patient-") {
        feed(&id);
    } else {
        panic!("no ihe-iti reader or writer takes this example");
    }
}

fn json(id: &str) -> Value {
    serde_json::from_str(&example(PACKAGE, id)).expect("JSON")
}

/// The example `id` in `format`.
fn body(id: &str, format: Format) -> Vec<u8> {
    let text = example(PACKAGE, id);
    if format == Format::Xml {
        let value: fhir_types::codec::Value = serde_json::from_str(&text).expect("JSON");
        let object: &Object = value.as_object().expect("a resource");
        return fhir_types::xml::to_xml(&fhir_types::r4::schema::SCHEMAS, object)
            .expect("an XML form")
            .into_bytes();
    }
    text.into_bytes()
}

/// The parameters of `parameters` named `name`.
fn named<'a>(parameters: &'a Value, name: &str) -> Vec<&'a Value> {
    parameters["parameter"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|parameter| parameter["name"] == name)
        .collect()
}

/// A posted ITI-83 request, read by the Manager in JSON and XML.
fn request(id: &str) {
    let example = json(id);
    let strings = |name: &str| -> Vec<String> {
        named(&example, name)
            .iter()
            .filter_map(|parameter| parameter["valueString"].as_str().map(str::to_owned))
            .collect()
    };
    let sources = strings("sourceIdentifier");
    let [source] = sources.as_slice() else {
        panic!("the example names {} sourceIdentifier", sources.len());
    };
    let (system, value) = source.split_once('|').expect("a system|value token");
    for format in [Format::Json, Format::Xml] {
        let query = PixQuery::from_body(format, &body(id, format))
            .unwrap_or_else(|error| panic!("the Manager refuses the {format:?} form: {error}"));
        assert_eq!(
            query.source().system(),
            system,
            "{format:?}: the source system"
        );
        assert_eq!(
            query.source().value().expose_secret(),
            value,
            "{format:?}: the source value"
        );
        let targets: Vec<&str> = query.targets().iter().map(TargetSystem::as_str).collect();
        assert_eq!(targets, strings("targetSystem"), "{format:?}: the targets");
        assert_eq!(
            query.format(),
            strings("_format").first().map(String::as_str),
            "{format:?}: _format"
        );
    }
}

/// The `(system, value)` of each `targetIdentifier` and the reference of each
/// `targetId` of a response.
fn out_parameters(example: &Value) -> (Vec<(String, String)>, Vec<String>) {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    let identifiers = named(example, "targetIdentifier")
        .iter()
        .map(|parameter| {
            let identifier = &parameter["valueIdentifier"];
            (text(&identifier["system"]), text(&identifier["value"]))
        })
        .collect();
    let references = named(example, "targetId")
        .iter()
        .map(|parameter| text(&parameter["valueReference"]["reference"]))
        .collect();
    (identifiers, references)
}

/// The parameters of a `Parameters` resource, in a fixed order.
fn sorted(parameters: &Value) -> Vec<Value> {
    let mut found: Vec<Value> = parameters["parameter"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    found.sort_by_key(Value::to_string);
    found
}

/// An ITI-83 answer, read by the client and written by the Manager.
async fn response(id: &str) {
    let example = json(id);
    let (identifiers, references) = out_parameters(&example);
    let targets: Vec<TargetSystem> = identifiers
        .iter()
        .map(|(system, _)| target(system))
        .collect();
    let server = manager(200, FHIR_JSON, example_text(id)).await;
    let answer = client(&server)
        .cross_reference(&red_source(), &targets, &OnBehalfOf::System, PROMPT)
        .await;
    let Ok(CrossReference::Matched(found)) = answer else {
        panic!("the client does not read it as a cross-reference: {answer:?}");
    };
    let read: Vec<(String, String)> = found
        .identifiers()
        .iter()
        .map(|identifier| {
            (
                identifier.system().to_owned(),
                identifier.value().expose_secret().to_owned(),
            )
        })
        .collect();
    assert_eq!(read, identifiers, "the client reads the identifiers");
    assert_eq!(
        found.patients().len(),
        references.len(),
        "and the targetIds"
    );

    let mut query = format!("sourceIdentifier={RED}%7C{RED_VALUE}");
    for target in &targets {
        query.push_str("&targetSystem=");
        query.push_str(target.as_str());
    }
    let query = PixQuery::from_query(&query).expect("the request the example answers");
    let answer = cross_references(
        &query,
        &identifiers
            .iter()
            .map(|(system, value)| {
                TargetIdentifier::new(system, SecretString::from(value.as_str()))
                    .expect("an identifier")
            })
            .collect::<Vec<_>>(),
        &references
            .iter()
            .map(|reference| {
                PatientReference::new(SecretString::from(reference.as_str())).expect("a reference")
            })
            .collect::<Vec<_>>(),
    );
    assert_eq!(answer.status(), StatusCode::OK, "the Manager's status");
    let written: Value =
        serde_json::from_slice(&answer.encode(Format::Json).expect("JSON")).expect("JSON");
    assert_eq!(
        sorted(&written),
        sorted(&example),
        "the Manager writes the out parameters the example holds, in any order (§2:3.83.4.2.2.1)"
    );
}

fn example_text(id: &str) -> String {
    example(PACKAGE, id)
}

/// The not-found answer, read by the client and written by the Manager.
async fn not_found(id: &str) {
    let server = manager(404, FHIR_JSON, example_text(id)).await;
    let answer = client(&server)
        .cross_reference(&red_source(), &[target(BLUE)], &OnBehalfOf::System, PROMPT)
        .await;
    assert!(
        matches!(answer, Ok(CrossReference::SourceNotFound)),
        "the client does not read it as source-not-found (§2:3.83.4.2.2.2): {answer:?}"
    );
    let refusal = Refusal::SourceIdentifierNotFound.answer();
    assert_eq!(
        refusal.status(),
        StatusCode::NOT_FOUND,
        "the Manager's status"
    );
    let written: Value =
        serde_json::from_slice(&refusal.encode(Format::Json).expect("JSON")).expect("JSON");
    let example = json(id);
    for key in ["severity", "code"] {
        assert_eq!(
            written["issue"][0][key], example["issue"][0][key],
            "the Manager writes the issue {key} the example holds"
        );
    }
}

/// A Patient of the Patient Identity Feed, read by the Manager as the
/// conditional update its first identifier names, in JSON and XML.
fn feed(id: &str) {
    let example = json(id);
    let identifier = &example["identifier"][0];
    let (Some(system), Some(value)) = (identifier["system"].as_str(), identifier["value"].as_str())
    else {
        panic!(
            "the Patient carries no identifier, so no ITI-104 conditional update names it (§2:3.104.4.1.2)"
        );
    };
    let condition = format!("identifier={system}%7C{value}");
    let merged = example["link"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|link| link["type"] == "replaced-by");
    for format in [Format::Json, Format::Xml] {
        let feed = Feed::from_update(&condition, format, &body(id, format))
            .unwrap_or_else(|error| panic!("the Manager refuses the {format:?} form: {error}"));
        match feed {
            Feed::ResolveDuplicate(_) if merged => {}
            Feed::Revise(_) if !merged => {}
            other => panic!("the Manager reads the {format:?} form as {other:?}"),
        }
    }
}
