// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Every answer ITI-78 defines, and the ones it does not (§2:3.78.4.1.3,
//! §2:3.78.4.2.2).

use std::time::Duration;

use http::StatusCode;
use ihe_iti::outcome::IssueType;
use ihe_iti::pdqm::error::{Malformation, PdqmError};
use ihe_iti::pdqm::matches::{MatchGrade, SearchResult};
use ihe_iti::pdqm::query::PatientQuery;
use ihe_iti::user::OnBehalfOf;
use secrecy::ExposeSecret;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{
    DOMAIN, EXAMPLE_BUNDLE, EXAMPLE_MAIDEN_NAME, EXAMPLE_PATIENT, FHIR_JSON, PROMPT, SEARCH,
    client, entry, outcome, schmidt, searchset, supplier, unreachable_client, vendored,
};
use crate::timing;

async fn ask(server: &MockServer, query: &PatientQuery) -> Result<SearchResult, PdqmError> {
    client(server)
        .search(query, &OnBehalfOf::System, PROMPT)
        .await
}

fn found(answer: Result<SearchResult, PdqmError>) -> SearchResult {
    match answer {
        Ok(found) => found,
        Err(error) => panic!("a result set, got {error:?}"),
    }
}

fn malformation(answer: Result<SearchResult, PdqmError>) -> Malformation {
    match answer {
        Err(PdqmError::Malformed(malformation)) => malformation,
        other => panic!("a malformed answer, got {other:?}"),
    }
}

fn domain_query() -> PatientQuery {
    schmidt()
        .identifier_domains(&[DOMAIN])
        .expect("a domain filter")
}

#[tokio::test]
async fn the_igs_example_bundle_is_one_match() {
    let server = supplier(200, FHIR_JSON, vendored(EXAMPLE_BUNDLE)).await;
    let found = found(ask(&server, &schmidt()).await);
    assert_eq!(found.total(), 1, "the example's total (Case 1)");
    let [matched] = found.patients() else {
        panic!("one Patient, got {}", found.patients().len());
    };
    assert_eq!(
        matched.full_url().expose_secret(),
        "http://example.org/Patient/ex-patient",
        "the entry's fullUrl"
    );
    assert_eq!(
        matched
            .patient()
            .birth_date
            .as_ref()
            .and_then(|date| date.value.as_deref()),
        Some("1923-07-25"),
        "the Patient decodes as FHIR R4"
    );
    assert_eq!(matched.score(), None, "the example conveys no score");
    assert_eq!(matched.grade(), None, "nor a match grade");
    assert!(found.next().is_none(), "a self link is no next page");
    assert!(found.warnings().is_empty(), "no OperationOutcome entry");
}

#[tokio::test]
async fn no_match_is_a_total_of_zero() {
    let server = supplier(200, FHIR_JSON, searchset(0, &[], &[])).await;
    let found = found(ask(&server, &schmidt()).await);
    assert_eq!(found.total(), 0, "the zero result set (Case 3)");
    assert!(found.patients().is_empty(), "no Patient");
}

#[tokio::test]
async fn several_matches_keep_their_order_score_and_grade() {
    let grade = |code: &str, score: &str| {
        format!(
            r#"{{"extension":[{{"url":"http://hl7.org/fhir/StructureDefinition/match-grade","valueCode":"{code}"}}],"mode":"match","score":{score}}}"#
        )
    };
    let body = searchset(
        2,
        &[
            entry(
                "http://example.org/Patient/ex-patient",
                &vendored(EXAMPLE_PATIENT),
                Some(&grade("probable", "0.9")),
            ),
            entry(
                "http://example.org/Patient/ex-patient-mothers-maiden-name",
                &vendored(EXAMPLE_MAIDEN_NAME),
                Some(&grade("possible", "0.6")),
            ),
        ],
        &[],
    );
    let server = supplier(200, FHIR_JSON, body).await;
    let found = found(ask(&server, &schmidt()).await);
    let ranked: Vec<_> = found
        .patients()
        .iter()
        .map(|matched| {
            (
                matched.patient().id.clone(),
                matched.score(),
                matched.grade(),
            )
        })
        .collect();
    assert_eq!(
        ranked,
        [
            (
                Some("ex-patient".to_owned()),
                Some(0.9),
                Some(MatchGrade::Probable)
            ),
            (
                Some("ex-patient-mothers-maiden-name".to_owned()),
                Some(0.6),
                Some(MatchGrade::Possible)
            ),
        ],
        "the Supplier's order, with its quality of match (§2:3.78.4.2.2.5)"
    );
}

#[tokio::test]
async fn a_deprecated_patient_is_a_match_marked_inactive() {
    let patient = r#"{"resourceType":"Patient","id":"old","active":false,"link":[{"other":{"reference":"Patient/new"},"type":"replaced-by"}]}"#;
    let body = searchset(
        1,
        &[entry("http://example.org/Patient/old", patient, None)],
        &[],
    );
    let server = supplier(200, FHIR_JSON, body).await;
    let found = found(ask(&server, &schmidt()).await);
    let [matched] = found.patients() else {
        panic!("one Patient");
    };
    assert_eq!(
        matched
            .patient()
            .active
            .as_ref()
            .and_then(|active| active.value),
        Some(false),
        "Case 6: the Patient with active set to false"
    );
}

#[tokio::test]
async fn an_unrecognised_domain_is_a_404_with_not_found() {
    let body = outcome("warning", "not-found", "targetSystem not found");
    let server = supplier(404, FHIR_JSON, body).await;
    assert!(
        matches!(
            ask(&server, &domain_query()).await,
            Err(PdqmError::DomainNotRecognized)
        ),
        "Case 4, the preferred answer"
    );
}

#[tokio::test]
async fn a_404_for_a_query_naming_no_domain_is_rejected() {
    let body = outcome("warning", "not-found", "targetSystem not found");
    let server = supplier(404, FHIR_JSON, body).await;
    match ask(&server, &schmidt()).await {
        Err(PdqmError::Rejected { status, issues }) => {
            assert_eq!(status, StatusCode::NOT_FOUND, "the status");
            assert_eq!(issues, [IssueType::NotFound], "the issue code");
        }
        other => panic!("a rejection, got {other:?}"),
    }
}

#[tokio::test]
async fn an_unrecognised_domain_may_be_a_200_with_a_warning() {
    let warning = outcome("warning", "not-found", "targetSystem not found");
    let body = searchset(
        0,
        &[entry(
            "urn:uuid:9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
            &warning,
            Some(r#"{"mode":"outcome"}"#),
        )],
        &[],
    );
    let server = supplier(200, FHIR_JSON, body).await;
    let found = found(ask(&server, &domain_query()).await);
    assert_eq!(
        found.warnings(),
        [IssueType::NotFound],
        "Case 4, the acceptable 200"
    );
    assert!(found.patients().is_empty(), "no Patient");
}

#[tokio::test]
async fn an_unsupported_format_is_rejected_with_its_issue() {
    let server = supplier(406, FHIR_JSON, outcome("error", "not-supported", "json")).await;
    match ask(&server, &schmidt()).await {
        Err(PdqmError::Rejected { status, issues }) => {
            assert_eq!(status, StatusCode::NOT_ACCEPTABLE, "Case 5");
            assert_eq!(issues, [IssueType::NotSupported], "the issue code");
        }
        other => panic!("a rejection, got {other:?}"),
    }
}

#[tokio::test]
async fn a_server_error_without_an_outcome_is_rejected() {
    let server = supplier(500, "text/html", "<html>down</html>").await;
    match ask(&server, &schmidt()).await {
        Err(PdqmError::Rejected { status, issues }) => {
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "the status");
            assert!(issues.is_empty(), "no OperationOutcome");
        }
        other => panic!("a rejection, got {other:?}"),
    }
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(
            ResponseTemplate::new(307).insert_header("location", "https://elsewhere.example/"),
        )
        .mount(&server)
        .await;
    assert!(
        matches!(
            ask(&server, &schmidt()).await,
            Err(PdqmError::Rejected { status, .. }) if status == StatusCode::TEMPORARY_REDIRECT
        ),
        "a 307 would replay the criteria elsewhere"
    );
}

#[tokio::test]
async fn a_slow_supplier_times_out() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(ResponseTemplate::new(200).set_delay(timing::SILENT))
        .mount(&server)
        .await;
    let client = client(&server);
    let limit = Duration::from_millis(200);
    let answer =
        timing::bounded(limit, client.search(&schmidt(), &OnBehalfOf::System, limit)).await;
    assert!(
        matches!(answer, Err(PdqmError::Timeout)),
        "a timeout, got {answer:?}"
    );
}

#[tokio::test]
async fn an_unreachable_supplier_is_a_transport_error() {
    let answer = unreachable_client()
        .search(&schmidt(), &OnBehalfOf::System, PROMPT)
        .await;
    assert!(
        matches!(answer, Err(PdqmError::Transport(_))),
        "a transport error, got {answer:?}"
    );
}

#[tokio::test]
async fn an_answer_that_is_no_fhir_json_bundle_is_malformed() {
    let cases = [
        (
            "text/plain",
            searchset(0, &[], &[]),
            Malformation::NotFhirJson,
        ),
        (
            FHIR_JSON,
            "{\"resourceType\"".to_owned(),
            Malformation::NotJson {
                line: 1,
                column: 15,
            },
        ),
        (FHIR_JSON, "[]".to_owned(), Malformation::NotAResource),
        (
            FHIR_JSON,
            outcome("information", "informational", "ok"),
            Malformation::UnexpectedResource,
        ),
    ];
    for (media, body, expected) in cases {
        let server = supplier(200, media, body).await;
        assert_eq!(
            malformation(ask(&server, &schmidt()).await),
            expected,
            "{media}"
        );
    }
}

#[tokio::test]
async fn a_bundle_off_the_response_profile_is_malformed() {
    let patient = r#"{"resourceType":"Patient","id":"p"}"#;
    let observation = r#"{"resourceType":"Observation","status":"final","code":{"text":"x"}}"#;
    let cases = [
        (
            r#"{"resourceType":"Bundle","type":"batch-response","total":0}"#.to_owned(),
            Malformation::NotSearchset,
        ),
        (
            r#"{"resourceType":"Bundle","type":"searchset"}"#.to_owned(),
            Malformation::NoTotal,
        ),
        (
            format!(
                r#"{{"resourceType":"Bundle","type":"searchset","total":1,"entry":[{{"resource":{patient}}}]}}"#
            ),
            Malformation::NoFullUrl { index: 0 },
        ),
        (
            r#"{"resourceType":"Bundle","type":"searchset","total":1,"entry":[{"fullUrl":"http://example.org/Patient/p"}]}"#.to_owned(),
            Malformation::NoResource { index: 0 },
        ),
        (
            searchset(1, &[entry("http://example.org/Observation/o", observation, None)], &[]),
            Malformation::UnexpectedEntry { index: 0 },
        ),
        (
            searchset(
                1,
                &[entry(
                    "http://example.org/Patient/p",
                    patient,
                    Some(r#"{"extension":[{"url":"http://hl7.org/fhir/StructureDefinition/match-grade","valueCode":"likely"}]}"#),
                )],
                &[],
            ),
            Malformation::MatchGrade { index: 0 },
        ),
        (
            searchset(
                0,
                &[entry("http://example.org/Patient/p", patient, None)],
                &[],
            ),
            Malformation::TotalBelowMatches,
        ),
    ];
    for (body, expected) in cases {
        let server = supplier(200, FHIR_JSON, body).await;
        assert_eq!(
            malformation(ask(&server, &schmidt()).await),
            expected,
            "the Query Patient Resource Response Message profile"
        );
    }
}

#[tokio::test]
async fn a_patient_off_fhir_r4_is_malformed() {
    let patient = r#"{"resourceType":"Patient","birthDate":"25-07-1923"}"#;
    let body = searchset(
        1,
        &[entry("http://example.org/Patient/p", patient, None)],
        &[],
    );
    let server = supplier(200, FHIR_JSON, body).await;
    assert!(
        matches!(
            malformation(ask(&server, &schmidt()).await),
            Malformation::Decode { .. }
        ),
        "a birthDate that is no FHIR date"
    );
}

#[tokio::test]
async fn an_answer_beyond_the_limit_is_refused() {
    let padding = "x".repeat(9 << 20);
    let body = format!(r#"{{"resourceType":"Bundle","id":"{padding}"}}"#);
    let server = supplier(200, FHIR_JSON, body).await;
    assert!(
        matches!(
            malformation(ask(&server, &schmidt()).await),
            Malformation::TooLarge { .. }
        ),
        "an answer longer than the client reads"
    );
}
