// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Every answer ITI-83 defines, and the ones it does not (§2:3.83.4.2.2).

use std::time::Duration;

use http::StatusCode;
use ihe_iti::outcome::IssueType;
use ihe_iti::pixm::error::{Malformation, PixmError};
use ihe_iti::pixm::identifier::{CrossReference, CrossReferences};
use ihe_iti::user::OnBehalfOf;
use secrecy::ExposeSecret;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{
    BLUE, FHIR_JSON, GREEN, OPERATION, PROMPT, client, manager, outcome, red_source, target,
    unreachable_client, vendored,
};
use crate::timing;

const ALL: &str = "example/Parameters-pixm-response-mohralice-red-all.json";
const TO_BLUE: &str = "example/Parameters-pixm-response-mohralice-red-to-blue.json";
const NOT_FOUND: &str = "example/OperationOutcome-pixm-response-error-not-found.json";

async fn ask(server: &MockServer, targets: &[&str]) -> Result<CrossReference, PixmError> {
    let targets: Vec<_> = targets.iter().map(|system| target(system)).collect();
    client(server)
        .cross_reference(&red_source(), &targets, &OnBehalfOf::System, PROMPT)
        .await
}

fn matched(answer: Result<CrossReference, PixmError>) -> CrossReferences {
    match answer {
        Ok(CrossReference::Matched(found)) => found,
        other => panic!("a cross-reference, got {other:?}"),
    }
}

fn malformation(answer: Result<CrossReference, PixmError>) -> Malformation {
    match answer {
        Err(PixmError::Malformed(malformation)) => malformation,
        other => panic!("a malformed answer, got {other:?}"),
    }
}

fn identifiers(found: &CrossReferences) -> Vec<(String, String)> {
    found
        .identifiers()
        .iter()
        .map(|identifier| {
            (
                identifier.system().to_owned(),
                identifier.value().expose_secret().to_owned(),
            )
        })
        .collect()
}

#[tokio::test]
async fn every_domain_answers_when_no_target_is_named() {
    let server = manager(200, FHIR_JSON, vendored(ALL)).await;
    let found = matched(ask(&server, &[]).await);
    assert_eq!(
        identifiers(&found),
        [
            (BLUE.to_owned(), "IHEBLUE-994".to_owned()),
            (GREEN.to_owned(), "IHEGREEN-994".to_owned()),
        ],
        "the IG's red-all example, in the Manager's order (§2:3.83.4.2.2.1)"
    );
    assert_eq!(
        found.patients().len(),
        2,
        "one targetId per matching Patient"
    );
}

#[tokio::test]
async fn several_targets_return_an_identifier_each() {
    let server = manager(200, FHIR_JSON, vendored(ALL)).await;
    let found = matched(ask(&server, &[BLUE, GREEN]).await);
    assert_eq!(
        found.in_domain(BLUE).count(),
        1,
        "the blue domain's identifier"
    );
    assert_eq!(
        found.in_domain(GREEN).count(),
        1,
        "the green domain's identifier"
    );
}

#[tokio::test]
async fn one_target_returns_its_identifier() {
    let server = manager(200, FHIR_JSON, vendored(TO_BLUE)).await;
    let found = matched(ask(&server, &[BLUE]).await);
    assert_eq!(
        identifiers(&found),
        [(BLUE.to_owned(), "IHEBLUE-994".to_owned())],
        "the IG's red-to-blue example"
    );
}

#[tokio::test]
async fn a_known_patient_with_no_identifier_in_the_domains_is_an_empty_match() {
    let server = manager(200, FHIR_JSON, r#"{"resourceType":"Parameters"}"#).await;
    let found = matched(ask(&server, &[BLUE]).await);
    assert!(
        found.identifiers().is_empty() && found.patients().is_empty(),
        "zero or more identifiers (§2:3.83.1)"
    );
}

#[tokio::test]
async fn the_not_found_answer_is_a_404_with_a_not_found_issue() {
    let server = manager(404, FHIR_JSON, vendored(NOT_FOUND)).await;
    assert!(
        matches!(
            ask(&server, &[BLUE]).await,
            Ok(CrossReference::SourceNotFound)
        ),
        "the IG's not-found example (§2:3.83.4.2.2.2)"
    );
}

#[tokio::test]
async fn a_404_without_a_not_found_issue_says_nothing_about_the_patient() {
    let server = manager(404, "text/html", "<html>no such route</html>").await;
    assert!(
        matches!(
            ask(&server, &[BLUE]).await,
            Err(PixmError::Rejected { status: StatusCode::NOT_FOUND, ref issues }) if issues.is_empty()
        ),
        "a misrouted request is an error, never a patient unknown"
    );
    let server = manager(404, FHIR_JSON, outcome("processing", "proxy")).await;
    assert!(
        matches!(
            ask(&server, &[BLUE]).await,
            Err(PixmError::Rejected {
                status: StatusCode::NOT_FOUND,
                ..
            })
        ),
        "only the not-found issue type marks the patient unknown"
    );
}

#[tokio::test]
async fn an_empty_bundle_after_a_merge_is_source_not_found() {
    let server = manager(
        200,
        FHIR_JSON,
        r#"{"resourceType":"Bundle","type":"searchset"}"#,
    )
    .await;
    assert!(
        matches!(
            ask(&server, &[BLUE]).await,
            Ok(CrossReference::SourceNotFound)
        ),
        "the post-merge or post-delete answer (§2:3.83.4.2.2.5)"
    );
    let server = manager(
        200,
        FHIR_JSON,
        r#"{"resourceType":"Bundle","type":"searchset","entry":[{"fullUrl":"Patient/1"}]}"#,
    )
    .await;
    assert_eq!(
        malformation(ask(&server, &[BLUE]).await),
        Malformation::BundleWithEntries,
        "a Bundle holding a Patient is not that answer"
    );
}

#[tokio::test]
async fn an_unknown_source_domain_is_a_400_with_code_invalid() {
    let server = manager(
        400,
        FHIR_JSON,
        outcome(
            "code-invalid",
            "sourceIdentifier Assigning Authority not found",
        ),
    )
    .await;
    assert!(
        matches!(
            ask(&server, &[BLUE]).await,
            Err(PixmError::SourceDomainNotRecognized)
        ),
        "§2:3.83.4.2.2.3"
    );
}

#[tokio::test]
async fn an_unknown_target_domain_is_a_403_with_code_invalid() {
    let server = manager(
        403,
        FHIR_JSON,
        outcome("code-invalid", "targetSystem not found"),
    )
    .await;
    assert!(
        matches!(
            ask(&server, &[BLUE]).await,
            Err(PixmError::TargetDomainNotRecognized)
        ),
        "§2:3.83.4.2.2.4"
    );
}

#[tokio::test]
async fn any_other_failure_is_rejected_with_its_issue_types() {
    let server = manager(500, FHIR_JSON, outcome("exception", "database down")).await;
    assert!(
        matches!(
            ask(&server, &[BLUE]).await,
            Err(PixmError::Rejected { status: StatusCode::INTERNAL_SERVER_ERROR, ref issues })
                if issues == &[IssueType::Exception]
        ),
        "a server failure"
    );
    let server = manager(302, "text/plain", "").await;
    assert!(
        matches!(
            ask(&server, &[BLUE]).await,
            Err(PixmError::Rejected {
                status: StatusCode::FOUND,
                ..
            })
        ),
        "a redirect is not followed and is an error (the client is built without redirects)"
    );
}

#[tokio::test]
async fn a_manager_that_does_not_answer_in_time_is_a_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(OPERATION))
        .respond_with(ResponseTemplate::new(200).set_delay(timing::SILENT))
        .mount(&server)
        .await;
    let client = client(&server);
    let limit = Duration::from_millis(200);
    let answer = timing::bounded(
        limit,
        client.cross_reference(&red_source(), &[target(BLUE)], &OnBehalfOf::System, limit),
    )
    .await;
    assert!(matches!(answer, Err(PixmError::Timeout)), "got {answer:?}");
}

#[tokio::test]
async fn an_unreachable_manager_is_a_transport_failure() {
    let answer = unreachable_client()
        .cross_reference(&red_source(), &[target(BLUE)], &OnBehalfOf::System, PROMPT)
        .await;
    assert!(
        matches!(answer, Err(PixmError::Transport(_))),
        "got {answer:?}"
    );
}

#[tokio::test]
async fn an_identifier_from_a_domain_not_asked_about_is_refused() {
    let server = manager(200, FHIR_JSON, vendored(ALL)).await;
    assert_eq!(
        malformation(ask(&server, &[BLUE]).await),
        Malformation::UnaskedDomain { index: 3 },
        "the green identifier was not asked for (§2:3.83.4.1.2.2)"
    );
}

#[tokio::test]
async fn the_source_identifier_returned_is_refused() {
    let body = format!(
        r#"{{"resourceType":"Parameters","parameter":[{{"name":"targetIdentifier","valueIdentifier":{{"system":"{}","value":"{}"}}}}]}}"#,
        super::RED,
        super::RED_VALUE
    );
    let server = manager(200, FHIR_JSON, body).await;
    assert_eq!(
        malformation(ask(&server, &[]).await),
        Malformation::SourceEchoed { index: 0 },
        "the source identifier shall not be included (§2:3.83.4.2.2.1)"
    );
}

#[tokio::test]
async fn an_answer_off_the_operation_definition_is_refused() {
    let cases = [
        (
            r#"{"resourceType":"Parameters","parameter":[{"name":"other","valueString":"x"}]}"#,
            Malformation::UnexpectedParameter { index: 0 },
            "an out parameter $ihe-pix does not define",
        ),
        (
            r#"{"resourceType":"Parameters","parameter":[{"name":"targetIdentifier","valueString":"x"}]}"#,
            Malformation::NotAnIdentifier { index: 0 },
            "a targetIdentifier is an Identifier",
        ),
        (
            r#"{"resourceType":"Parameters","parameter":[{"name":"targetIdentifier","valueIdentifier":{"value":"1"}}]}"#,
            Malformation::NoAssigningAuthority { index: 0 },
            "an identifier carries its assigning authority (ITI TF-2 Appendix E.3)",
        ),
        (
            r#"{"resourceType":"Parameters","parameter":[{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.1"}}]}"#,
            Malformation::NoIdentifierValue { index: 0 },
            "an identifier carries a value",
        ),
        (
            r#"{"resourceType":"Parameters","parameter":[{"name":"targetId","valueReference":{"display":"x"}}]}"#,
            Malformation::NoReference { index: 0 },
            "a targetId is a Reference with a reference",
        ),
        (
            r#"{"resourceType":"Parameters","parameter":[{"name":"targetId","part":[{"name":"x","valueString":"y"}]}]}"#,
            Malformation::UnexpectedShape { index: 0 },
            "no out parameter has parts",
        ),
        (
            r#"{"resourceType":"OperationOutcome","issue":[{"severity":"information","code":"informational"}]}"#,
            Malformation::UnexpectedResource,
            "a 200 answers with Parameters",
        ),
        (
            r#"["not","a","resource"]"#,
            Malformation::NotAResource,
            "a resource is a JSON object",
        ),
        (
            "{",
            Malformation::NotJson { line: 1, column: 1 },
            "a JSON syntax error",
        ),
    ];
    for (body, expected, why) in cases {
        let server = manager(200, FHIR_JSON, body).await;
        assert_eq!(malformation(ask(&server, &[]).await), expected, "{why}");
    }
}

#[tokio::test]
async fn an_answer_that_is_not_fhir_json_or_too_long_is_refused() {
    let server = manager(200, "text/html", vendored(ALL)).await;
    assert_eq!(
        malformation(ask(&server, &[]).await),
        Malformation::NotFhirJson,
        "ITI TF-2 Appendix Z.6"
    );
    let server = manager(200, "application/fhir+json; charset=utf-8", vendored(ALL)).await;
    assert!(
        matches!(ask(&server, &[]).await, Ok(CrossReference::Matched(_))),
        "a charset parameter is still FHIR JSON"
    );
    let padded = format!(
        r#"{{"resourceType":"Parameters","id":"{}"}}"#,
        "x".repeat(2 << 20)
    );
    let server = manager(200, FHIR_JSON, padded).await;
    assert!(
        matches!(
            malformation(ask(&server, &[]).await),
            Malformation::TooLarge { .. }
        ),
        "the client stops reading at its limit"
    );
}

#[tokio::test]
async fn a_resource_that_does_not_decode_is_refused() {
    let server = manager(
        200,
        FHIR_JSON,
        r#"{"resourceType":"Parameters","unknown":true}"#,
    )
    .await;
    assert!(
        matches!(
            malformation(ask(&server, &[]).await),
            Malformation::Decode { .. }
        ),
        "an unknown property is not FHIR R4"
    );
}
