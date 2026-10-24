// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Manager side of ITI-83 and ITI-104 (feature `pixm-server`), read and
//! written over the vendored PIXm 3.1.0 examples, in JSON and XML.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the vendored examples are read as JSON values for their identifiers"
)]

use std::fmt::Debug;

use fhir_types::codec::{Json, Object, Path, Value};
use fhir_types::r4::parameters::{Parameters, ParametersParameterValue};
use http::StatusCode;
use ihe_iti::outcome::IssueType;
use ihe_iti::pixm::identifier::{PatientReference, TargetIdentifier};
use ihe_iti::pixm::server::Refusal;
use ihe_iti::pixm::server::answer::cross_references;
use ihe_iti::pixm::server::feed::{Feed, FeedError, Removal};
use ihe_iti::pixm::server::query::{PixQuery, QueryError};
use ihe_iti::server::format::Format;
use secrecy::{ExposeSecret, SecretString};

use super::{BLUE, GREEN, RED, RED_VALUE, vendored};
use crate::display::displays;

/// A value that must appear in no `Debug` rendering.
const SENTINEL: &str = "SENTINEL-4711";

/// The Mohr Alice examples of the PIXm package an ITI-104 feed carries.
const MOHR_ALICE: &[&str] = &[
    "example/Patient-Patient-MaidenAlice-Red.json",
    "example/Patient-Patient-MohrAlice-Blue.json",
    "example/Patient-Patient-MohrAlice-Green.json",
    "example/Patient-Patient-MohrAlice-Red.json",
    "example/Patient-Patient-MohrAlice.json",
    "example/Patient-Patient-MohrAlissa-Red.json",
    "example/Patient-Patient-MohrMaidenResolvedByMohrMalice-Red.json",
];

/// The resource `json` holds, as FHIR XML.
fn xml(json: &str) -> String {
    let value: Value = serde_json::from_str(json).expect("JSON");
    let object: &Object = value.as_object().expect("a resource");
    fhir_types::xml::to_xml(&fhir_types::r4::schema::SCHEMAS, object).expect("an XML form")
}

/// The vendored example `file` in `format`.
fn body(file: &str, format: Format) -> Vec<u8> {
    let json = vendored(file);
    match format {
        Format::Xml => xml(&json).into_bytes(),
        _ => json.into_bytes(),
    }
}

/// The `Parameters` of `parameters`, each a `valueString`, as FHIR JSON.
fn posted(parameters: &[(&str, &str)]) -> Vec<u8> {
    let parameter: Vec<String> = parameters
        .iter()
        .map(|(name, value)| format!(r#"{{"name":"{name}","valueString":"{value}"}}"#))
        .collect();
    format!(
        r#"{{"resourceType":"Parameters","parameter":[{}]}}"#,
        parameter.join(",")
    )
    .into_bytes()
}

fn targets(query: &PixQuery) -> Vec<&str> {
    query
        .targets()
        .iter()
        .map(ihe_iti::pixm::identifier::TargetSystem::as_str)
        .collect()
}

#[test]
fn the_get_and_both_posted_encodings_read_into_one_request() {
    let get = PixQuery::from_query(&format!(
        "sourceIdentifier={RED}%7C{RED_VALUE}&targetSystem={BLUE}&targetSystem={GREEN}&_format=application/fhir%2Bxml"
    ))
    .expect("the GET of §2:3.83.4.1.2");
    for format in [Format::Json, Format::Xml] {
        let post = PixQuery::from_body(
            format,
            &body(
                "example/Parameters-pixm-request-mohralice-red-to-blue.json",
                format,
            ),
        )
        .expect("the IG's posted request");
        assert_eq!(post.source().system(), get.source().system(), "{format:?}");
        assert_eq!(
            post.source().value().expose_secret(),
            get.source().value().expose_secret(),
            "{format:?}"
        );
        assert_eq!(targets(&post), targets(&get), "{format:?}");
        assert_eq!(post.format(), get.format(), "{format:?}");
    }
    let all = PixQuery::from_body(
        Format::Json,
        &body(
            "example/Parameters-pixm-request-mohralice-red-all.json",
            Format::Json,
        ),
    )
    .expect("the IG's request for every domain");
    assert!(
        all.targets().is_empty(),
        "no targetSystem asks for every domain"
    );
}

/// A refusal: its name, the query of the `GET`, the body of the `POST`, the
/// error, and the status and issue type of its answer.
type Case = (
    &'static str,
    String,
    Vec<u8>,
    QueryError,
    StatusCode,
    IssueType,
);

#[test]
fn each_refusal_of_either_form_carries_its_outcome() {
    let source = format!("{RED}|{RED_VALUE}");
    let cases: [Case; 3] = [
        (
            "a second sourceIdentifier",
            format!("sourceIdentifier={RED}%7C1&sourceIdentifier={RED}%7C2"),
            posted(&[("sourceIdentifier", &source), ("sourceIdentifier", &source)]),
            QueryError::SecondSourceIdentifier,
            StatusCode::BAD_REQUEST,
            IssueType::Invalid,
        ),
        (
            "a token without a system",
            format!("sourceIdentifier={RED_VALUE}"),
            posted(&[("sourceIdentifier", RED_VALUE)]),
            QueryError::NoAssigningAuthority,
            StatusCode::BAD_REQUEST,
            IssueType::Invalid,
        ),
        (
            "an unknown parameter",
            format!("sourceIdentifier={RED}%7C1&patient=1"),
            posted(&[("sourceIdentifier", &source), ("patient", "1")]),
            QueryError::UnknownParameter,
            StatusCode::BAD_REQUEST,
            IssueType::NotSupported,
        ),
    ];
    for (case, query, parameters, error, status, code) in cases {
        let from_get = PixQuery::from_query(&query).expect_err(case);
        let from_post = PixQuery::from_body(Format::Json, &parameters).expect_err(case);
        for refused in [from_get, from_post] {
            assert_eq!(refused, error, "{case}");
            let answer = refused.refusal();
            assert_eq!(answer.status(), status, "{case}");
            assert_eq!(answer.issue_type(), code, "{case}");
            let outcome =
                String::from_utf8(answer.encode(Format::Json).expect("JSON")).expect("UTF-8");
            assert!(
                outcome.contains(r#""resourceType":"OperationOutcome""#),
                "{case}"
            );
        }
    }
}

#[test]
fn the_store_refusals_are_the_ones_the_transaction_prescribes() {
    let not_found = Refusal::SourceIdentifierNotFound.answer();
    let json = String::from_utf8(not_found.encode(Format::Json).expect("JSON")).expect("UTF-8");
    assert_eq!(
        json,
        r#"{"issue":[{"code":"not-found","diagnostics":"sourceIdentifier Patient Identifier not found","severity":"error"}],"resourceType":"OperationOutcome"}"#,
        "§2:3.83.4.2.2.2"
    );
    assert_eq!(not_found.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        Refusal::SourceDomainNotRecognized.answer().status(),
        StatusCode::BAD_REQUEST,
        "§2:3.83.4.2.2.3"
    );
    assert_eq!(
        Refusal::TargetDomainNotRecognized.answer().status(),
        StatusCode::FORBIDDEN,
        "§2:3.83.4.2.2.4"
    );
}

/// The `(name, system, value, reference)` of each parameter, sorted.
fn parameters(parameters: &Parameters) -> Vec<(String, String, String, String)> {
    let mut found: Vec<(String, String, String, String)> = parameters
        .parameter
        .iter()
        .map(|parameter| {
            let name = parameter.name.value.clone().unwrap_or_default();
            match &parameter.value {
                Some(ParametersParameterValue::Identifier(identifier)) => (
                    name,
                    identifier
                        .system
                        .as_ref()
                        .and_then(|s| s.value.clone())
                        .unwrap_or_default(),
                    identifier
                        .value
                        .as_ref()
                        .and_then(|v| v.value.clone())
                        .unwrap_or_default(),
                    String::new(),
                ),
                Some(ParametersParameterValue::Reference(reference)) => (
                    name,
                    String::new(),
                    String::new(),
                    reference
                        .reference
                        .as_ref()
                        .and_then(|r| r.value.clone())
                        .unwrap_or_default(),
                ),
                _ => (name, String::new(), String::new(), String::new()),
            }
        })
        .collect();
    found.sort();
    found
}

#[test]
fn the_answer_holds_what_the_vendored_response_holds() {
    let example: Value = serde_json::from_str(&vendored(
        "example/Parameters-pixm-response-mohralice-red-all.json",
    ))
    .expect("JSON");
    let example = Parameters::from_json(
        example.as_object().expect("a resource"),
        &mut Path::root("Parameters"),
    )
    .expect("the IG's response");
    let query = PixQuery::from_query(&format!("sourceIdentifier={RED}%7C{RED_VALUE}"))
        .expect("the IG's request");
    let identifier = |system: &str, value: &str| {
        TargetIdentifier::new(system, SecretString::from(value)).expect("an identifier")
    };
    let reference =
        |text: &str| PatientReference::new(SecretString::from(text)).expect("a reference");
    let answer = cross_references(
        &query,
        &[
            identifier(RED, RED_VALUE),
            identifier(BLUE, "IHEBLUE-994"),
            identifier(GREEN, "IHEGREEN-994"),
        ],
        &[
            reference("Patient/Patient-MohrAlice-Blue"),
            reference("Patient/Patient-MohrAlice-Green"),
        ],
    );
    assert_eq!(answer.status(), StatusCode::OK);
    assert_eq!(
        parameters(answer.resource()),
        parameters(&example),
        "the same out parameters, in any order (§2:3.83.4.2.2.1), the source left out"
    );
}

#[test]
fn every_vendored_feed_example_is_read_in_both_encodings() {
    for file in MOHR_ALICE {
        let json: serde_json::Value = serde_json::from_str(&vendored(file)).expect("JSON");
        let identifier = &json["identifier"][0];
        let condition = format!(
            "identifier={}%7C{}",
            identifier["system"].as_str().expect("a system"),
            identifier["value"].as_str().expect("a value")
        );
        for format in [Format::Json, Format::Xml] {
            let feed = Feed::from_update(&condition, format, &body(file, format))
                .unwrap_or_else(|error| panic!("{file} in {format:?}: {error}"));
            let merged = file.contains("ResolvedBy");
            match feed {
                Feed::ResolveDuplicate(merge) if merged => {
                    assert_eq!(merge.subsumed().value().expose_secret(), "IHERED-m94");
                    assert_eq!(merge.survivor().value().expose_secret(), RED_VALUE);
                    assert_eq!(merge.survivor().system(), RED, "§2:3.104.4.2.2");
                }
                Feed::Revise(revision) if !merged => {
                    assert_eq!(
                        revision.identifier().system(),
                        identifier["system"].as_str().expect("a system")
                    );
                }
                other => panic!("{file}: {other:?}"),
            }
        }
    }
}

#[test]
fn a_body_whose_identifier_is_not_the_condition_is_refused() {
    let body = body("example/Patient-Patient-MohrAlice-Blue.json", Format::Json);
    let refused = Feed::from_update(
        &format!("identifier={RED}%7C{RED_VALUE}"),
        Format::Json,
        &body,
    )
    .expect_err("the body carries the blue identifier only");
    assert_eq!(refused, FeedError::ConditionNotCarried, "§2:3.104.4.1.2");
    assert_eq!(refused.refusal().status(), StatusCode::BAD_REQUEST);
    let no_identifier = Feed::from_update(
        &format!("identifier={RED}%7C{RED_VALUE}"),
        Format::Json,
        vendored("example/Patient-ex-patient.json").as_bytes(),
    )
    .expect_err("a Patient without an identifier");
    assert_eq!(no_identifier, FeedError::Profile, "§2:3.104.4.1.2.1");
}

#[test]
fn the_vendored_remove_patient_condition_is_read() {
    let removal = Removal::from_delete(&format!("identifier={RED}%7C{RED_VALUE}"))
        .expect("the Remove Patient example of §2:3.104.4.3.5");
    assert_eq!(removal.identifier().system(), RED);
    assert_eq!(removal.identifier().value().expose_secret(), RED_VALUE);
}

#[test]
fn no_type_with_an_identifier_displays_or_shows_it() {
    let query =
        PixQuery::from_query(&format!("sourceIdentifier={RED}%7C{SENTINEL}")).expect("a query");
    let patient = format!(
        r#"{{"resourceType":"Patient","identifier":[{{"system":"{RED}","value":"{SENTINEL}"}}],"name":[{{"family":"{SENTINEL}"}}],"link":[{{"type":"replaced-by","other":{{"identifier":{{"system":"{RED}","value":"{SENTINEL}-2"}}}}}}]}}"#
    );
    let condition = format!("identifier={RED}%7C{SENTINEL}");
    let merge = Feed::from_update(&condition, Format::Json, patient.as_bytes()).expect("a merge");
    let revision_body = patient.replace("replaced-by", "seealso");
    let revision =
        Feed::from_update(&condition, Format::Json, revision_body.as_bytes()).expect("a revision");
    let removal = Removal::from_delete(&condition).expect("a removal");
    let answer = cross_references(
        &query,
        &[TargetIdentifier::new(BLUE, SecretString::from(SENTINEL)).expect("an identifier")],
        &[PatientReference::new(SecretString::from(SENTINEL)).expect("a reference")],
    );
    let shown: [&dyn Debug; 5] = [&query, &merge, &revision, &removal, &answer];
    for rendering in shown {
        let text = format!("{rendering:?}");
        assert!(!text.contains(SENTINEL), "Debug showed a value: {text}");
    }
    assert!(!displays!(PixQuery), "PixQuery");
    assert!(!displays!(Feed), "Feed");
    assert!(!displays!(Removal), "Removal");
    assert!(
        !displays!(ihe_iti::pixm::server::feed::Revision),
        "Revision"
    );
    assert!(!displays!(ihe_iti::pixm::server::feed::Merge), "Merge");
    assert!(
        !displays!(ihe_iti::server::answer::Answer<Parameters>),
        "Answer"
    );
}
