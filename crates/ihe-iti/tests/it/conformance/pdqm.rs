// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The PDQm 3.2.0 corpus: each example of the vendored package but the audit
//! records, read by the ITI-78 and ITI-119 client or the Supplier side and,
//! where the Supplier writes it, written back.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the vendored examples and the written answers are compared as JSON values"
)]

use fhir_types::codec::{Json, Object, Path};
use fhir_types::r4::bundle::Bundle;
use fhir_types::r4::operation_outcome::OperationOutcome;
use fhir_types::r4::patient::Patient;
use fhir_types::r4::resource::Resource;
use http::StatusCode;
use ihe_atna::user::OnBehalfOf;
use ihe_iti::pdqm::error::PdqmError;
use ihe_iti::pdqm::input::MatchInput;
use ihe_iti::pdqm::matches::MatchGrade;
use ihe_iti::pdqm::server::matching::{MatchEntry, MatchRequest, match_set};
use ihe_iti::pdqm::server::search::PatientSearch;
use ihe_iti::pdqm::server::searchset::{Paging, SearchEntry, searchset};
use ihe_iti::pdqm::server::{Score, retrieved};
use ihe_iti::server::format::Format;
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{example, examples, is_audit, record, verdict};
use crate::pdqm::{DOMAIN, FHIR_JSON, PROMPT, client, entry, schmidt, supplier};

/// The vendored package under `docs/specs/`.
const PACKAGE: &str = "ihe-pdqm";

/// The match path under the stub's FHIR base.
const MATCH: &str = "/fhir/Patient/$match";

/// The Supplier's `[base]/Patient` the search answer is written for.
const ENDPOINT: &str = "https://pdq.example.org/fhir/Patient";

/// The match input example the Supplier writes a match answer for.
const PATIENT_ONLY: &str = "example/Parameters-ex-match-input-patient-only.json";

#[tokio::test]
async fn conformance_the_pdqm_examples_hold_their_pass_list() {
    let mut cases = Vec::new();
    for id in examples(PACKAGE).into_iter().filter(|id| !is_audit(id)) {
        cases.push(verdict(id.clone(), check(id)).await);
    }
    record(
        "pdqm",
        "The PDQm 3.2.0 examples of docs/specs/ihe-pdqm, read and written through ihe-iti",
        cases,
    );
}

/// Checks the example `id` by the resource and the profile it holds.
async fn check(id: String) {
    let name = id.trim_start_matches("example/");
    let held = json(&id);
    let profile = held["meta"]["profile"][0].as_str().unwrap_or_default();
    if name.starts_with("Bundle-") && profile.ends_with("QueryPatientResourceResponseMessage") {
        search_answer(&id).await;
    } else if name.starts_with("Bundle-") && profile.ends_with("MatchParametersOut") {
        match_answer(&id).await;
    } else if name.starts_with("Parameters-") {
        match_input(&id);
    } else if name.starts_with("Patient-") && profile.ends_with("MatchInputPatient") {
        bare_match_input(&id);
    } else if name.starts_with("Patient-") {
        retrieve(&id).await;
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

/// The Bundle of the example `id`.
fn bundle(id: &str) -> Bundle {
    let value: fhir_types::codec::Value =
        serde_json::from_str(&example(PACKAGE, id)).expect("JSON");
    Bundle::from_json(
        value.as_object().expect("a resource"),
        &mut Path::root("Bundle"),
    )
    .expect("an R4 Bundle")
}

/// The `(fullUrl, Patient)` of each Patient entry of `bundle`.
fn patients(bundle: &Bundle) -> Vec<(Url, Patient)> {
    bundle
        .entry
        .iter()
        .filter_map(|entry| match &entry.resource {
            Some(Resource::Patient(patient)) => Some((
                Url::parse(entry.full_url.as_ref()?.value.as_deref()?).expect("a fullUrl"),
                (**patient).clone(),
            )),
            _ => None,
        })
        .collect()
}

/// The `OperationOutcome` entries of `bundle`.
fn outcomes(bundle: &Bundle) -> Vec<OperationOutcome> {
    bundle
        .entry
        .iter()
        .filter_map(|entry| match &entry.resource {
            Some(Resource::OperationOutcome(outcome)) => Some((**outcome).clone()),
            _ => None,
        })
        .collect()
}

/// The grade a `match-grade` code names.
fn grade(code: &str) -> MatchGrade {
    match code {
        "certain" => MatchGrade::Certain,
        "probable" => MatchGrade::Probable,
        "possible" => MatchGrade::Possible,
        "certainly-not" => MatchGrade::CertainlyNot,
        other => panic!("{other} is no match-grade code"),
    }
}

/// The `(grade, score)` of each Patient entry of the example `held`.
fn graded(held: &Value) -> Vec<(MatchGrade, f64)> {
    held["entry"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["resource"]["resourceType"] == "Patient")
        .map(|entry| {
            let search = &entry["search"];
            let code = search["extension"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|extension| {
                    extension["url"] == "http://hl7.org/fhir/StructureDefinition/match-grade"
                })
                .and_then(|extension| extension["valueCode"].as_str())
                .expect("a match-grade");
            (grade(code), search["score"].as_f64().expect("a score"))
        })
        .collect()
}

/// An ITI-78 answer, read by the client and written by the Supplier.
async fn search_answer(id: &str) {
    let example = bundle(id);
    let server = supplier(200, FHIR_JSON, example_text(id)).await;
    let found = client(&server)
        .search(&schmidt(), &OnBehalfOf::System, PROMPT)
        .await
        .unwrap_or_else(|error| panic!("the client refuses it: {error:?}"));
    let expected = patients(&example);
    assert_eq!(
        Some(found.total()),
        example.total.as_ref().and_then(|total| total.value),
        "the client reads the total"
    );
    let read: Vec<&str> = found
        .patients()
        .iter()
        .map(|matched| matched.full_url().expose_secret())
        .collect();
    let listed: Vec<&str> = expected.iter().map(|(url, _)| url.as_str()).collect();
    assert_eq!(read, listed, "the client reads each entry");

    let held = json(id);
    let self_link = held["link"][0]["url"].as_str().expect("a self link");
    let (_, query) = self_link
        .split_once('?')
        .expect("a search in the self link");
    let search = PatientSearch::from_query(query).expect("the example's search");
    let entries = expected
        .iter()
        .cloned()
        .map(|(full_url, patient)| SearchEntry::matched(full_url, patient))
        .collect();
    let total = found.total();
    let answer = searchset(
        &search,
        &Url::parse(ENDPOINT).expect("a URL"),
        total,
        entries,
        &Paging::new(),
    )
    .unwrap_or_else(|error| panic!("the Supplier cannot write it: {error}"));
    let written = answer.resource();
    assert_eq!(
        written.total, example.total,
        "the Supplier writes the total"
    );
    assert_eq!(patients(written), expected, "and each entry");
}

fn example_text(id: &str) -> String {
    example(PACKAGE, id)
}

/// A stub Supplier that answers every match with `status` and the example.
async fn matcher(status: u16, id: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(MATCH))
        .respond_with(
            ResponseTemplate::new(status).set_body_raw(example_text(id).into_bytes(), FHIR_JSON),
        )
        .mount(&server)
        .await;
    server
}

/// An ITI-119 answer: a failure Bundle of outcomes alone read by the client
/// as the failure it reports (§2:3.119.4.1.3, Case 9), any other read as a
/// `200` and written by the Supplier.
async fn match_answer(id: &str) {
    let example = bundle(id);
    let held = json(id);
    let input = MatchInput::new(DOMAIN, &SecretString::from("12345")).expect("an input");
    if patients(&example).is_empty() && !outcomes(&example).is_empty() {
        let server = matcher(500, id).await;
        let answer = client(&server)
            .match_patient(&input, &OnBehalfOf::System, PROMPT)
            .await;
        let Err(PdqmError::Rejected { status, issues }) = answer else {
            panic!("the client does not read it as a failure: {answer:?}");
        };
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!issues.is_empty(), "the client keeps the issue types");
        return;
    }
    let server = matcher(200, id).await;
    let found = client(&server)
        .match_patient(&input, &OnBehalfOf::System, PROMPT)
        .await
        .unwrap_or_else(|error| panic!("the client refuses it as a 200: {error:?}"));
    let expected = graded(&held);
    let read: Vec<(Option<MatchGrade>, Option<f64>)> = found
        .patients()
        .iter()
        .map(|matched| (matched.grade(), matched.score()))
        .collect();
    let listed: Vec<(Option<MatchGrade>, Option<f64>)> = expected
        .iter()
        .map(|(grade, score)| (Some(*grade), Some(*score)))
        .collect();
    assert_eq!(read, listed, "the client reads each match");

    let request =
        MatchRequest::from_request("", Format::Json, example_text(PATIENT_ONLY).as_bytes())
            .expect("the Patient-only input");
    let entries = patients(&example)
        .into_iter()
        .zip(&expected)
        .map(|((full_url, patient), (grade, score))| {
            MatchEntry::new(
                full_url,
                patient,
                Score::new(*score).expect("a score"),
                *grade,
            )
        })
        .collect();
    let self_link = held["link"][0]["url"]
        .as_str()
        .map(|url| Url::parse(url).expect("a self link"));
    let answer = match_set(&request, entries, outcomes(&example), self_link.as_ref())
        .unwrap_or_else(|error| panic!("the Supplier cannot write it: {error}"));
    let written = Bundle {
        id: example.id.clone(),
        meta: example.meta.clone(),
        ..answer.resource().clone()
    };
    assert_eq!(
        serde_json::to_value(&written).expect("JSON"),
        held,
        "the Supplier writes every element and value the example holds"
    );
}

/// An ITI-119 input `Parameters`, read by the Supplier in JSON and XML.
fn match_input(id: &str) {
    let held = json(id);
    let parameter = |name: &str| {
        held["parameter"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|parameter| parameter["name"] == name)
            .cloned()
    };
    let certain = parameter("onlyCertainMatches")
        .and_then(|parameter| parameter["valueBoolean"].as_bool())
        .unwrap_or(false);
    let count = parameter("count").and_then(|parameter| parameter["valueInteger"].as_u64());
    for format in [Format::Json, Format::Xml] {
        let request = MatchRequest::from_request("", format, &body(id, format))
            .unwrap_or_else(|error| panic!("the Supplier refuses the {format:?} form: {error}"));
        assert_eq!(
            request.only_certain_matches(),
            certain,
            "{format:?}: onlyCertainMatches"
        );
        assert_eq!(
            request.count().map(|count| u64::from(count.get())),
            count,
            "{format:?}: count"
        );
    }
}

/// A bare `$match` input Patient, read by the Supplier in JSON and XML
/// (§2:3.119.4.1.2).
fn bare_match_input(id: &str) {
    for format in [Format::Json, Format::Xml] {
        MatchRequest::from_request("", format, &body(id, format))
            .unwrap_or_else(|error| panic!("the Supplier refuses the {format:?} form: {error}"));
    }
}

/// A Patient a Supplier answers with: read by the client from a search
/// answer and written by the Supplier as the answer of a retrieve.
async fn retrieve(id: &str) {
    let text = example_text(id);
    let held = json(id);
    let full_url = format!(
        "http://example.org/Patient/{}",
        held["id"].as_str().expect("an id")
    );
    let bundle = crate::pdqm::searchset(
        1,
        &[entry(&full_url, &text, Some(r#"{"mode":"match"}"#))],
        &[],
    );
    let server = supplier(200, FHIR_JSON, bundle).await;
    let found = client(&server)
        .search(&schmidt(), &OnBehalfOf::System, PROMPT)
        .await
        .unwrap_or_else(|error| panic!("the client refuses it in a search answer: {error:?}"));
    let [matched] = found.patients() else {
        panic!("the client reads {} Patients", found.patients().len());
    };
    let answer = retrieved(matched.patient().clone());
    assert_eq!(answer.status(), StatusCode::OK, "the Supplier's status");
    let written: Value =
        serde_json::from_slice(&answer.encode(Format::Json).expect("JSON")).expect("JSON");
    assert_eq!(
        written, held,
        "the Supplier writes every element and value the example holds"
    );
}
