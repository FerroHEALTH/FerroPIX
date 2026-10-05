// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Every answer Mitz can give, read into a decision per category or a typed
//! error: only `Permit` and `Deny` are decisions (Implementatiehandleiding
//! Open en gesloten autorisatievraag 3.8.2 §3.2.4.6, §3.2.5; Programma van
//! Eisen AMC AUS-TR-e0050, AUS-TR-e0900).

use std::time::Duration;

use http::StatusCode;
use nl_generic_functions::mitz::error::{FaultCode, Malformation, MitzError};
use nl_generic_functions::mitz::question::{DataCategory, Decision};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{
    CATEGORIES, HOLDER, PATIENT, PROMPT, SOAP_XML, answer, client, decided, fault, mitz, question,
    result,
};

fn category(code: &str) -> DataCategory {
    DataCategory::new(code).expect("a data category")
}

async fn failure(server: &MockServer) -> MitzError {
    client(server)
        .ask(&question(), PROMPT)
        .await
        .expect_err("no decision")
}

async fn answered(results: &[String]) -> MitzError {
    failure(&mitz(200, SOAP_XML, &answer(results)).await).await
}

#[tokio::test]
async fn a_permit_and_a_deny_are_one_decision_per_category() {
    let body = answer(&[
        decided("Permit", CATEGORIES[0]),
        decided("Deny", CATEGORIES[1]),
    ]);
    let server = mitz(200, SOAP_XML, &body).await;
    let decided = client(&server)
        .ask(&question(), PROMPT)
        .await
        .expect("a decision per category");
    assert_eq!(
        Some(Decision::Permit),
        decided.decision(&category(CATEGORIES[0]))
    );
    assert_eq!(
        Some(Decision::Deny),
        decided.decision(&category(CATEGORIES[1]))
    );
    assert!(!decided.denies_all());
}

#[tokio::test]
async fn a_deny_for_every_category_denies_all() {
    let body = answer(&[
        decided("Deny", CATEGORIES[1]),
        decided("Deny", CATEGORIES[0]),
    ]);
    let server = mitz(200, SOAP_XML, &body).await;
    let decided = client(&server)
        .ask(&question(), PROMPT)
        .await
        .expect("a decision per category");
    assert!(decided.denies_all(), "the results may come in any order");
}

#[tokio::test]
async fn indeterminate_is_an_error_and_never_a_decision() {
    let error = answered(&[
        decided("Permit", CATEGORIES[0]),
        decided("Indeterminate", CATEGORIES[1]),
    ])
    .await;
    assert!(
        matches!(&error, MitzError::Indeterminate { category: seen } if *seen == category(CATEGORIES[1])),
        "{error:?}"
    );
    assert_eq!(Some(StatusCode::OK), error.status());
}

#[tokio::test]
async fn not_applicable_is_an_error_and_never_a_decision() {
    let error = answered(&[
        decided("NotApplicable", CATEGORIES[0]),
        decided("Permit", CATEGORIES[1]),
    ])
    .await;
    assert!(
        matches!(error, MitzError::NotApplicable { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_unknown_decision_is_malformed() {
    let error = answered(&[
        decided("Allow", CATEGORIES[0]),
        decided("Permit", CATEGORIES[1]),
    ])
    .await;
    assert!(
        matches!(
            error,
            MitzError::Malformed(Malformation::UnknownDecision { result: 0 })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_category_without_a_result_is_malformed() {
    let error = answered(&[decided("Permit", CATEGORIES[0])]).await;
    assert!(
        matches!(&error, MitzError::Malformed(Malformation::MissingResult(missing)) if *missing == category(CATEGORIES[1])),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_category_answered_twice_is_malformed() {
    let error = answered(&[
        decided("Permit", CATEGORIES[0]),
        decided("Deny", CATEGORIES[0]),
        decided("Permit", CATEGORIES[1]),
    ])
    .await;
    assert!(
        matches!(
            error,
            MitzError::Malformed(Malformation::DuplicateResult { result: 1 })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_category_not_asked_about_is_malformed() {
    let error = answered(&[
        decided("Permit", "GGC999"),
        decided("Permit", CATEGORIES[1]),
    ])
    .await;
    assert!(
        matches!(
            error,
            MitzError::Malformed(Malformation::UnaskedCategory { result: 0 })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_result_about_another_patient_is_never_a_decision() {
    let error = answered(&[
        result("Permit", CATEGORIES[0], "bsn-synthetic-0002", HOLDER),
        decided("Permit", CATEGORIES[1]),
    ])
    .await;
    assert!(
        matches!(
            error,
            MitzError::Malformed(Malformation::OtherPatient { result: 0 })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_result_about_another_holder_is_never_a_decision() {
    let error = answered(&[
        decided("Permit", CATEGORIES[0]),
        result("Permit", CATEGORIES[1], PATIENT, "ura-test-0002"),
    ])
    .await;
    assert!(
        matches!(
            error,
            MitzError::Malformed(Malformation::OtherHolder { result: 1 })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_result_with_two_decisions_is_malformed() {
    let twice = decided("Permit", CATEGORIES[0]).replacen(
        "<Decision>Permit</Decision>",
        "<Decision>Permit</Decision><Decision>Deny</Decision>",
        1,
    );
    let error = answered(&[twice, decided("Permit", CATEGORIES[1])]).await;
    assert!(
        matches!(
            error,
            MitzError::Malformed(Malformation::NoDecision { result: 0 })
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_body_without_a_response_is_malformed() {
    let body = answer(&[]).replace("Response", "Answer");
    let error = failure(&mitz(200, SOAP_XML, &body).await).await;
    assert!(
        matches!(error, MitzError::Malformed(Malformation::NoResponse)),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_soap_fault_is_typed_with_its_code_and_status() {
    let server = mitz(500, SOAP_XML, &fault("s:Receiver", "synthetic outage")).await;
    let error = failure(&server).await;
    assert!(
        matches!(
            error,
            MitzError::Fault {
                code: FaultCode::Receiver,
                status: StatusCode::INTERNAL_SERVER_ERROR
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_status_without_a_soap_message_is_rejected() {
    let server = mitz(503, "text/plain", "synthetic outage").await;
    let error = failure(&server).await;
    assert!(
        matches!(
            error,
            MitzError::Rejected {
                status: StatusCode::SERVICE_UNAVAILABLE
            }
        ),
        "{error:?}"
    );
    assert_eq!(Some(StatusCode::SERVICE_UNAVAILABLE), error.status());
}

#[tokio::test]
async fn a_redirect_is_never_followed() {
    let elsewhere = mitz(200, SOAP_XML, &answer(&[decided("Permit", CATEGORIES[0])])).await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/elsewhere", elsewhere.uri())),
        )
        .mount(&server)
        .await;
    let error = failure(&server).await;
    assert!(
        matches!(
            error,
            MitzError::Rejected {
                status: StatusCode::TEMPORARY_REDIRECT
            }
        ),
        "{error:?}"
    );
    let followed = elsewhere
        .received_requests()
        .await
        .expect("recorded requests");
    assert!(
        followed.is_empty(),
        "the BSN went where the redirect pointed"
    );
}

#[tokio::test]
async fn a_200_that_is_not_soap_is_malformed() {
    let server = mitz(200, "application/json", "{}").await;
    let error = failure(&server).await;
    assert!(
        matches!(error, MitzError::Malformed(Malformation::NotSoap)),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_answer_that_is_not_xml_is_malformed() {
    let server = mitz(200, SOAP_XML, "<s:Envelope").await;
    let error = failure(&server).await;
    assert!(
        matches!(
            error,
            MitzError::Malformed(Malformation::NotXml { .. } | Malformation::NotAnEnvelope)
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_document_type_is_refused() {
    let body = format!(
        "<!DOCTYPE s:Envelope [<!ENTITY x \"x\">]>{}",
        answer(&[decided("Permit", CATEGORIES[0])])
    );
    let error = failure(&mitz(200, SOAP_XML, &body).await).await;
    assert!(
        matches!(error, MitzError::Malformed(Malformation::DocumentType)),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_answer_nested_too_deep_is_refused() {
    let deep = format!("{}{}", "<x>".repeat(80), "</x>".repeat(80));
    let body = answer(&[deep]);
    let error = failure(&mitz(200, SOAP_XML, &body).await).await;
    assert!(
        matches!(error, MitzError::Malformed(Malformation::TooDeep { .. })),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_answer_longer_than_the_bound_is_refused() {
    let padding = " ".repeat(2 * 1024 * 1024);
    let body = answer(&[padding]);
    let error = failure(&mitz(200, SOAP_XML, &body).await).await;
    assert!(
        matches!(error, MitzError::Malformed(Malformation::TooLarge { .. })),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_silent_mitz_times_out() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
        .mount(&server)
        .await;
    let error = client(&server)
        .ask(&question(), Duration::from_millis(200))
        .await
        .expect_err("no decision");
    assert!(matches!(error, MitzError::Timeout), "{error:?}");
    assert_eq!(None, error.status());
}
