// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-119, Patient Demographics Match, against a stub Supplier: the request
//! held to the `$match` `OperationDefinition` and the Match Input Parameters
//! profile, the IG's output examples read, the cases of §2:3.119.4.1.3, and
//! the demographics kept out of everything but the request body.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the request body and the vendored artefacts are read as JSON values"
)]

use std::num::NonZeroU16;

use ihe_iti::outcome::IssueType;
use ihe_iti::pdqm::error::{Malformation, PdqmError};
use ihe_iti::pdqm::input::MatchInput;
use ihe_iti::pdqm::matches::MatchGrade;
use secrecy::SecretString;
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{DOMAIN, FHIR_JSON, PROMPT, client, outcome, unreachable_client, vendored};

/// The match path under the stub's FHIR base.
const MATCH: &str = "/fhir/Patient/$match";

/// A value that must appear nowhere but in the request to the Supplier.
const SENTINEL: &str = "SENTINEL-4712";

/// A stub Supplier that answers every match with `status`, the media type
/// `media` and `body`.
async fn matcher(status: u16, media: &str, body: impl Into<String>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(MATCH))
        .respond_with(ResponseTemplate::new(status).set_body_raw(body.into().into_bytes(), media))
        .mount(&server)
        .await;
    server
}

fn input() -> MatchInput {
    MatchInput::new(DOMAIN, &SecretString::from(SENTINEL))
        .expect("an input")
        .only_certain_matches(true)
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).expect("JSON")
}

#[tokio::test]
async fn the_request_posts_a_parameters_resource_to_the_match_operation() {
    let server = matcher(
        200,
        FHIR_JSON,
        vendored("example/Bundle-ex-match-output.json"),
    )
    .await;
    client(&server)
        .match_patient(&input(), PROMPT)
        .await
        .expect("an answer");
    let requests = server.received_requests().await.expect("recorded requests");
    let [request] = requests.as_slice() else {
        panic!("one request, got {}", requests.len());
    };
    assert_eq!(request.method.as_str(), "POST", "§2:3.119.4.1.2");
    assert_eq!(request.url.path(), MATCH, "[base]/Patient/$match");
    assert_eq!(request.url.query(), None, "no criterion in the URL");
    assert_eq!(
        request
            .headers
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some(FHIR_JSON)
    );
    let body: Value = serde_json::from_slice(&request.body).expect("a JSON body");
    serde_json::from_slice::<fhir_types::r4::parameters::Parameters>(&request.body)
        .expect("the body reads back as an R4 Parameters");
    assert_eq!(body["resourceType"], "Parameters");
    let parameters = body["parameter"].as_array().expect("parameters");
    assert_eq!(parameters.len(), 2);
    assert_eq!(parameters[0]["name"], "resource");
    assert_eq!(parameters[0]["resource"]["resourceType"], "Patient");
    assert_eq!(
        parameters[0]["resource"]["identifier"][0],
        json(&format!(r#"{{"system":"{DOMAIN}","value":"{SENTINEL}"}}"#))
    );
    assert_eq!(parameters[1]["name"], "onlyCertainMatches");
    assert_eq!(parameters[1]["valueBoolean"], true);
}

#[tokio::test]
async fn the_request_has_the_shape_of_the_igs_only_certain_matches_example() {
    let server = matcher(
        200,
        FHIR_JSON,
        vendored("example/Bundle-ex-match-output.json"),
    )
    .await;
    client(&server)
        .match_patient(&input(), PROMPT)
        .await
        .expect("an answer");
    let requests = server.received_requests().await.expect("recorded requests");
    let body: Value = serde_json::from_slice(&requests[0].body).expect("a JSON body");
    let example = json(&vendored(
        "example/Parameters-ex-match-input-onlyCertainMatches.json",
    ));
    let shape = |parameters: &Value| -> Vec<(String, Vec<String>)> {
        parameters["parameter"]
            .as_array()
            .expect("parameters")
            .iter()
            .map(|parameter| {
                let mut keys: Vec<String> = parameter
                    .as_object()
                    .expect("a parameter")
                    .keys()
                    .cloned()
                    .collect();
                keys.sort();
                (parameter["name"].as_str().expect("a name").to_owned(), keys)
            })
            .collect()
    };
    assert_eq!(shape(&body), shape(&example));
}

#[tokio::test]
async fn the_parameters_profile_admits_the_three_parameters_the_input_sends() {
    let profile = json(&vendored(
        "StructureDefinition-IHE.PDQm.MatchParametersIn.json",
    ));
    let slices: Vec<&str> = profile["differential"]["element"]
        .as_array()
        .expect("elements")
        .iter()
        .filter_map(|element| element["sliceName"].as_str())
        .collect();
    assert_eq!(slices, ["resource", "onlyCertainMatches", "count"]);
    let server = matcher(
        200,
        FHIR_JSON,
        vendored("example/Bundle-ex-match-output.json"),
    )
    .await;
    let counted = input().count(NonZeroU16::new(5).expect("five"));
    client(&server)
        .match_patient(&counted, PROMPT)
        .await
        .expect("an answer");
    let requests = server.received_requests().await.expect("recorded requests");
    let body: Value = serde_json::from_slice(&requests[0].body).expect("a JSON body");
    assert_eq!(body["parameter"][2]["name"], "count");
    assert_eq!(body["parameter"][2]["valueInteger"], 5);
}

#[test]
fn the_supplier_capability_statement_declares_the_operation_the_client_invokes() {
    let statement = json(&vendored(
        "CapabilityStatement-IHE.PDQm.PatientDemographicsSupplierMatch.json",
    ));
    let operation = json(&vendored("OperationDefinition-PDQmMatch.json"));
    let resource = &statement["rest"][0]["resource"][0];
    assert_eq!(resource["type"], "Patient");
    assert_eq!(resource["operation"][0]["name"], "match");
    assert_eq!(resource["operation"][0]["definition"], operation["url"]);
    assert_eq!(operation["code"], "match");
    assert_eq!(operation["type"], true, "a type-level operation");
    assert_eq!(operation["instance"], false);
}

#[tokio::test]
async fn one_certain_match_is_read_with_its_score_and_grade() {
    let server = matcher(
        200,
        FHIR_JSON,
        vendored("example/Bundle-ex-match-output.json"),
    )
    .await;
    let found = client(&server)
        .match_patient(&input(), PROMPT)
        .await
        .expect("an answer");
    let [matched] = found.patients() else {
        panic!("one match (§2:3.119.4.1.3, Case 1)");
    };
    assert_eq!(matched.grade(), Some(MatchGrade::Certain));
    assert_eq!(matched.score(), Some(0.9));
    assert!(found.warnings().is_empty());
}

#[tokio::test]
async fn several_matches_are_read_most_likely_first() {
    let server = matcher(
        200,
        FHIR_JSON,
        vendored("example/Bundle-ex-match-output-multiple.json"),
    )
    .await;
    let found = client(&server)
        .match_patient(&input(), PROMPT)
        .await
        .expect("an answer");
    let grades: Vec<Option<MatchGrade>> = found
        .patients()
        .iter()
        .map(ihe_iti::pdqm::matches::MatchedPatient::grade)
        .collect();
    assert_eq!(
        grades,
        [Some(MatchGrade::Probable), Some(MatchGrade::Possible)],
        "§2:3.119.4.1.3, Case 2"
    );
}

#[tokio::test]
async fn no_match_is_an_answer_with_no_patient() {
    let server = matcher(
        200,
        FHIR_JSON,
        vendored("example/Bundle-ex-match-output-empty.json"),
    )
    .await;
    let found = client(&server)
        .match_patient(&input(), PROMPT)
        .await
        .expect("an answer (§2:3.119.4.1.3, Cases 5 and 7)");
    assert!(found.patients().is_empty());
}

#[tokio::test]
async fn an_error_outcome_in_a_successful_answer_is_refused() {
    // NOTE: §2:3.119.4.1.3 Case 10 says a warning outcome has no error or fatal
    // severity; the IG's warning example carries `error`, so the text decides.
    for example in [
        "example/Bundle-ex-match-output-warning.json",
        "example/Bundle-ex-match-output-error.json",
    ] {
        let server = matcher(200, FHIR_JSON, vendored(example)).await;
        let error = client(&server)
            .match_patient(&input(), PROMPT)
            .await
            .expect_err("an error outcome is no success");
        assert!(
            matches!(
                error,
                PdqmError::Malformed(Malformation::ErrorOutcome { .. })
            ),
            "{example}: {error:?}"
        );
    }
}

#[tokio::test]
async fn a_warning_outcome_is_kept_beside_the_match() {
    let patient = json(&vendored("example/Bundle-ex-match-output.json"))["entry"][0].clone();
    let bundle = format!(
        r#"{{"resourceType":"Bundle","type":"searchset","entry":[{patient},{{"fullUrl":"urn:uuid:7c2b8b9e-0000-4000-8000-000000000001","resource":{},"search":{{"mode":"outcome"}}}}]}}"#,
        outcome("warning", "informational", SENTINEL)
    );
    let server = matcher(200, FHIR_JSON, bundle).await;
    let found = client(&server)
        .match_patient(&input(), PROMPT)
        .await
        .expect("an answer (§2:3.119.4.1.3, Case 10)");
    assert_eq!(found.patients().len(), 1);
    assert_eq!(found.warnings(), [IssueType::Informational]);
}

#[tokio::test]
async fn a_failure_answer_keeps_the_issue_codes_of_a_bundle_of_outcomes() {
    let server = matcher(
        500,
        FHIR_JSON,
        vendored("example/Bundle-ex-match-output-error.json"),
    )
    .await;
    let error = client(&server)
        .match_patient(&input(), PROMPT)
        .await
        .expect_err("a failure (§2:3.119.4.1.3, Case 9)");
    let PdqmError::Rejected { status, issues } = error else {
        panic!("a rejection, got {error:?}");
    };
    assert_eq!(status, http::StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(issues, [IssueType::Timeout]);
}

#[tokio::test]
async fn a_patient_entry_without_its_score_grade_or_mode_is_refused() {
    let patient = r#"{"resourceType":"Patient","id":"p1"}"#;
    let grade =
        r#"{"url":"http://hl7.org/fhir/StructureDefinition/match-grade","valueCode":"certain"}"#;
    let cases = [
        (String::from(r#"{"mode":"match","score":0.9}"#), "no grade"),
        (
            format!(r#"{{"mode":"match","extension":[{grade}]}}"#),
            "no score",
        ),
        (
            format!(r#"{{"mode":"match","score":1.5,"extension":[{grade}]}}"#),
            "a score above 1",
        ),
        (
            format!(r#"{{"mode":"include","score":0.9,"extension":[{grade}]}}"#),
            "not a match",
        ),
    ];
    for (search, case) in cases {
        let bundle = format!(
            r#"{{"resourceType":"Bundle","type":"searchset","entry":[{{"fullUrl":"https://pdq.example.org/fhir/Patient/p1","resource":{patient},"search":{search}}}]}}"#
        );
        let server = matcher(200, FHIR_JSON, bundle).await;
        let error = client(&server)
            .match_patient(&input(), PROMPT)
            .await
            .expect_err("the Match Output Bundle profile refuses it");
        assert!(
            matches!(
                error,
                PdqmError::Malformed(
                    Malformation::NoMatchGrade { index: 0 }
                        | Malformation::NoScore { index: 0 }
                        | Malformation::NotMatchMode { index: 0 }
                )
            ),
            "{case}: {error:?}"
        );
    }
}

#[tokio::test]
async fn the_demographics_reach_the_supplier_in_the_body_only() {
    let server = matcher(404, FHIR_JSON, outcome("error", "not-found", SENTINEL)).await;
    let error = client(&server)
        .match_patient(&input(), PROMPT)
        .await
        .expect_err("a refusal");
    let requests = server.received_requests().await.expect("recorded requests");
    assert!(String::from_utf8_lossy(&requests[0].body).contains(SENTINEL));
    assert!(
        !requests[0].url.as_str().contains(SENTINEL),
        "not in the URL"
    );
    assert!(
        !format!("{error} {error:?} {:?}", input()).contains(SENTINEL),
        "no error and no Debug names the identifier"
    );
    let unreachable = unreachable_client()
        .match_patient(&input(), PROMPT)
        .await
        .expect_err("no Supplier answers");
    assert!(!format!("{unreachable} {unreachable:?}").contains(SENTINEL));
}

#[test]
fn an_input_refuses_a_system_that_is_no_uri_and_an_empty_value() {
    let invalid = ihe_iti::pdqm::error::InvalidInput::System;
    assert_eq!(
        MatchInput::new("not a uri", &SecretString::from("1")).err(),
        Some(invalid)
    );
    assert_eq!(
        MatchInput::new(DOMAIN, &SecretString::from("")).err(),
        Some(ihe_iti::pdqm::error::InvalidInput::EmptyValue)
    );
}
