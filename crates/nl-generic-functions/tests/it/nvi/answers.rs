// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What each answer of the Localization Service means: the custodians of a
//! `searchset` of localization records, every page included, and an error
//! for every answer that is not one, so an outage or a defective answer never
//! reads as a patient with no data anywhere.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the fixtures build FHIR JSON as values"
)]

use std::time::Duration;

use nl_generic_functions::nvi::error::{Malformation, NviError};
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{
    FHIR_JSON, PATIENT, PROMPT, SEARCH, client, patient, record, searchset, service,
    unreachable_client,
};

fn uras(localization: &nl_generic_functions::nvi::Localization) -> Vec<&str> {
    localization
        .custodians()
        .iter()
        .map(nl_generic_functions::identification::Ura::as_str)
        .collect()
}

async fn malformed(body: &Value) -> Malformation {
    let server = service(200, FHIR_JSON, body).await;
    match client(&server).localize(&patient(), PROMPT).await {
        Err(NviError::Malformed(malformation)) => malformation,
        other => panic!("a malformed answer, not {other:?}"),
    }
}

#[tokio::test]
async fn the_custodians_of_the_records_are_the_localization() {
    let body = searchset(
        vec![
            record(PATIENT, "ura-test-0002"),
            record(PATIENT, "ura-test-0001"),
            record(PATIENT, "ura-test-0002"),
        ],
        None,
    );
    let server = service(200, FHIR_JSON, &body).await;
    let localization = client(&server)
        .localize(&patient(), PROMPT)
        .await
        .expect("a localization");
    assert_eq!(
        uras(&localization),
        ["ura-test-0001", "ura-test-0002"],
        "each custodian once, in order"
    );
}

#[tokio::test]
async fn an_empty_searchset_names_no_custodian() {
    let server = service(200, FHIR_JSON, &searchset(Vec::new(), None)).await;
    let localization = client(&server)
        .localize(&patient(), PROMPT)
        .await
        .expect("a localization");
    assert!(localization.is_empty());
}

#[tokio::test]
async fn a_record_that_no_longer_stands_localizes_nothing() {
    let mut superseded = record(PATIENT, "ura-test-0003");
    superseded["status"] = json!("superseded");
    let mut erroneous = record(PATIENT, "ura-test-0004");
    erroneous["status"] = json!("entered-in-error");
    let body = searchset(
        vec![record(PATIENT, "ura-test-0001"), superseded, erroneous],
        None,
    );
    let server = service(200, FHIR_JSON, &body).await;
    let localization = client(&server)
        .localize(&patient(), PROMPT)
        .await
        .expect("a localization");
    assert_eq!(uras(&localization), ["ura-test-0001"]);
}

#[tokio::test]
async fn an_outcome_entry_tells_about_the_search_and_is_no_record() {
    let outcome = json!({
        "resourceType": "OperationOutcome",
        "issue": [{"severity": "information", "code": "informational"}]
    });
    let body = searchset(vec![outcome, record(PATIENT, "ura-test-0001")], None);
    let server = service(200, FHIR_JSON, &body).await;
    let localization = client(&server)
        .localize(&patient(), PROMPT)
        .await
        .expect("a localization");
    assert_eq!(uras(&localization), ["ura-test-0001"]);
}

#[tokio::test]
async fn every_page_is_read() {
    let server = MockServer::start().await;
    let next = format!("{}{SEARCH}?_page=2", server.uri());
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .and(query_param("_page", "2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                searchset(vec![record(PATIENT, "ura-test-0002")], None)
                    .to_string()
                    .into_bytes(),
                FHIR_JSON,
            ),
        )
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                searchset(vec![record(PATIENT, "ura-test-0001")], Some(&next))
                    .to_string()
                    .into_bytes(),
                FHIR_JSON,
            ),
        )
        .with_priority(2)
        .expect(1)
        .mount(&server)
        .await;
    let localization = client(&server)
        .localize(&patient(), PROMPT)
        .await
        .expect("a localization");
    assert_eq!(uras(&localization), ["ura-test-0001", "ura-test-0002"]);
}

#[tokio::test]
async fn a_next_link_off_the_endpoint_is_never_followed() {
    let body = searchset(
        vec![record(PATIENT, "ura-test-0001")],
        Some("https://elsewhere.example.org/fhir/DocumentReference?_page=2"),
    );
    assert_eq!(malformed(&body).await, Malformation::NextLink);
}

#[tokio::test]
async fn a_failure_status_is_an_error_carrying_it() {
    for status in [400, 401, 403, 404, 500, 503] {
        let outcome = json!({"resourceType": "OperationOutcome", "issue": []});
        let server = service(status, FHIR_JSON, &outcome).await;
        match client(&server).localize(&patient(), PROMPT).await {
            Err(NviError::Rejected { status: answered }) => {
                assert_eq!(answered.as_u16(), status);
            }
            other => panic!("{status} is a rejection, not {other:?}"),
        }
    }
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", "https://elsewhere.example.org/"),
        )
        .mount(&server)
        .await;
    assert!(matches!(
        client(&server).localize(&patient(), PROMPT).await,
        Err(NviError::Rejected { .. })
    ));
}

#[tokio::test]
async fn a_slow_service_is_a_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(searchset(Vec::new(), None).to_string(), FHIR_JSON)
                .set_delay(Duration::from_secs(5)),
        )
        .mount(&server)
        .await;
    assert!(matches!(
        client(&server)
            .localize(&patient(), Duration::from_millis(100))
            .await,
        Err(NviError::Timeout)
    ));
}

#[tokio::test]
async fn an_unreachable_service_is_a_transport_error() {
    let answer = unreachable_client().localize(&patient(), PROMPT).await;
    assert!(
        matches!(answer, Err(NviError::Transport(_) | NviError::Timeout)),
        "{answer:?}"
    );
}

#[tokio::test]
async fn an_answer_that_is_no_fhir_json_is_refused() {
    let server = service(200, "text/html", &searchset(Vec::new(), None)).await;
    assert!(matches!(
        client(&server).localize(&patient(), PROMPT).await,
        Err(NviError::Malformed(Malformation::NotFhirJson))
    ));
}

#[tokio::test]
async fn a_resource_other_than_a_searchset_bundle_is_refused() {
    let outcome = json!({"resourceType": "OperationOutcome", "issue": []});
    assert_eq!(malformed(&outcome).await, Malformation::NotABundle);
    let history = json!({"resourceType": "Bundle", "type": "history"});
    assert_eq!(malformed(&history).await, Malformation::NotASearchset);
}

#[tokio::test]
async fn a_record_about_another_subject_is_refused() {
    let other = searchset(vec![record("pbsn-synthetic-0099", "ura-test-0001")], None);
    assert_eq!(
        malformed(&other).await,
        Malformation::OtherSubject { index: 0 }
    );
    let mut by_bsn = record(PATIENT, "ura-test-0001");
    by_bsn["subject"]["identifier"]["system"] = json!("http://fhir.nl/fhir/NamingSystem/bsn");
    assert_eq!(
        malformed(&searchset(vec![by_bsn], None)).await,
        Malformation::OtherSubject { index: 0 },
        "a record naming the patient by anything but the pseudonym"
    );
}

#[tokio::test]
async fn a_record_of_another_type_is_refused() {
    let mut other = record(PATIENT, "ura-test-0001");
    other["type"]["coding"][0]["code"] = json!("11488-4");
    assert_eq!(
        malformed(&searchset(vec![other], None)).await,
        Malformation::OtherType { index: 0 }
    );
}

#[tokio::test]
async fn a_record_naming_no_custodian_by_ura_is_refused() {
    let mut other = record(PATIENT, "ura-test-0001");
    other["custodian"]["identifier"]["system"] = json!("http://fhir.nl/fhir/NamingSystem/kvk");
    assert_eq!(
        malformed(&searchset(vec![other], None)).await,
        Malformation::NoCustodian { index: 0 }
    );
    let mut none = record(PATIENT, "ura-test-0001");
    none.as_object_mut()
        .expect("a record object")
        .remove("custodian");
    assert_eq!(
        malformed(&searchset(vec![none], None)).await,
        Malformation::NoCustodian { index: 0 }
    );
}

#[tokio::test]
async fn an_entry_that_is_no_record_is_refused() {
    let patient = json!({"resourceType": "Patient", "id": "synthetic-1"});
    assert_eq!(
        malformed(&searchset(vec![patient], None)).await,
        Malformation::UnexpectedEntry { index: 0 }
    );
}
