// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The five cases of §3.55.4.2.3, the SOAP fault, and every answer that does
//! not hold to ITI-55, which is an error and never a discovery.

use std::time::Duration;

use ihe_iti::xcpd::discovery::{Discovery, RequestedAttribute};
use ihe_iti::xcpd::error::{DetectedIssue, FaultCode, Malformation, XcpdError};
use secrecy::ExposeSecret;
use wiremock::ResponseTemplate;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

use super::{
    PATH, PROMPT, SOAP_XML, Templated, answering, client, fixture, gateway, query, responding,
};

async fn ask(server: &MockServer) -> Result<Discovery, XcpdError> {
    client()
        .discover(&responding(server), &query(), None, PROMPT)
        .await
}

async fn answer_of(status: u16, media: &str, body: String) -> Result<Discovery, XcpdError> {
    ask(&gateway(Templated::new(status, media, body)).await).await
}

fn malformation(answer: Result<Discovery, XcpdError>) -> Option<Malformation> {
    match answer {
        Err(XcpdError::Malformed(found)) => Some(found),
        _ => None,
    }
}

#[tokio::test]
async fn case_1_one_match_names_its_community_and_patient_id() {
    let answer = ask(&answering("match.xml").await).await;
    let Ok(Discovery::Matched(found)) = answer else {
        panic!("Case 1 is a match: {answer:?}");
    };
    assert_eq!(1, found.len());
    let record = &found[0];
    assert_eq!("urn:oid:2.999.50", record.community().to_string());
    let id = &record.patient_ids()[0];
    assert_eq!("2.999.50.2", id.root().expose_secret());
    assert_eq!(
        Some("PID-50-0001"),
        id.extension().map(ExposeSecret::expose_secret)
    );
}

#[tokio::test]
async fn case_2_several_matches_name_each_community() {
    let answer = ask(&answering("matches.xml").await).await;
    let Ok(discovery) = answer else {
        panic!("Case 2 is a match: {answer:?}");
    };
    let communities: Vec<String> = discovery
        .communities()
        .into_iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(vec!["urn:oid:2.999.50", "urn:oid:2.999.60"], communities);
}

#[tokio::test]
async fn case_3_more_attributes_are_requested() {
    let answer = ask(&answering("more-attributes.xml").await).await;
    let Ok(Discovery::MoreAttributesRequested(requested)) = answer else {
        panic!("Case 3 asks for attributes: {answer:?}");
    };
    assert_eq!(
        vec![
            RequestedAttribute::PatientAddress,
            RequestedAttribute::PatientTelecom
        ],
        requested
    );
}

#[tokio::test]
async fn case_4_no_match_is_an_answer() {
    let answer = ask(&answering("no-match.xml").await).await;
    assert!(matches!(answer, Ok(Discovery::NoMatch)), "{answer:?}");
}

#[tokio::test]
async fn case_5_an_application_error_is_an_error_with_its_issue() {
    let answer = ask(&answering("application-error.xml").await).await;
    match answer {
        Err(XcpdError::ApplicationError { issues }) => {
            assert_eq!(vec![DetectedIssue::ResponderBusy], issues);
        }
        other => panic!("Case 5 is an error, never a no-match: {other:?}"),
    }
}

#[tokio::test]
async fn a_soap_fault_is_an_error_with_its_code_only() {
    let answer = answer_of(500, SOAP_XML, fixture("fault.xml")).await;
    match answer {
        Err(XcpdError::Fault { code, status }) => {
            assert_eq!(FaultCode::Receiver, code);
            assert_eq!(http::StatusCode::INTERNAL_SERVER_ERROR, status);
        }
        other => panic!("a fault is a transmission error: {other:?}"),
    }
}

#[tokio::test]
async fn an_unreadable_answer_with_an_error_status_is_rejected_with_it() {
    let answer = answer_of(502, SOAP_XML, "<not-closed".to_owned()).await;
    assert!(
        matches!(answer, Err(XcpdError::Rejected { status }) if status == http::StatusCode::BAD_GATEWAY),
        "{answer:?}"
    );
}

#[tokio::test]
async fn every_error_names_the_status_the_gateway_answered_with() {
    let cases = [
        (
            answer_of(500, SOAP_XML, fixture("fault.xml")).await,
            Some(500),
        ),
        (answer_of(503, "text/plain", String::new()).await, Some(503)),
        (
            ask(&answering("application-error.xml").await).await,
            Some(200),
        ),
        (answer_of(200, "text/xml", String::new()).await, Some(200)),
    ];
    for (answer, expected) in cases {
        let status = answer.err().and_then(|error| error.status());
        assert_eq!(
            expected,
            status.map(|status| status.as_u16()),
            "the status the gateway answered with"
        );
    }
    let unreachable = client()
        .discover(
            &ihe_iti::xcpd::request::RespondingGateway::unencrypted_for_development(
                url::Url::parse("http://127.0.0.1:0/rg").expect("a URL"),
                super::oid(super::RECEIVER),
            )
            .expect("a gateway"),
            &query(),
            None,
            PROMPT,
        )
        .await;
    assert_eq!(
        None,
        unreachable.err().and_then(|error| error.status()),
        "no answer"
    );
}

#[tokio::test]
async fn a_status_without_a_soap_answer_is_rejected() {
    let answer = answer_of(503, "text/plain", "unavailable".to_owned()).await;
    assert!(
        matches!(answer, Err(XcpdError::Rejected { status }) if status == http::StatusCode::SERVICE_UNAVAILABLE),
        "{answer:?}"
    );
}

#[tokio::test]
async fn an_ok_answer_that_is_not_soap_is_malformed() {
    let answer = answer_of(200, "text/xml", fixture("match.xml")).await;
    assert_eq!(Some(Malformation::NotSoap), malformation(answer));
    let answer = answer_of(
        200,
        "multipart/related; type=\"application/xop+xml\"",
        String::new(),
    )
    .await;
    assert_eq!(Some(Malformation::Multipart), malformation(answer));
}

#[tokio::test]
async fn an_answer_to_another_request_is_refused() {
    let body = fixture("match.xml").replace(
        "{relates_to}",
        "urn:uuid:00000000-0000-4000-8000-000000000000",
    );
    let answer = ask(&gateway(Templated::new(200, SOAP_XML, body)).await).await;
    assert_eq!(Some(Malformation::RelatesToAnother), malformation(answer));
}

#[tokio::test]
async fn every_departure_from_the_response_contract_is_malformed() {
    let base = fixture("match.xml");
    let cases = [
        (
            base.replace(r#"<id root="2.999.50"/>"#, ""),
            Malformation::NoHomeCommunity { event: 0 },
        ),
        (
            base.replace(r#"<id root="2.999.50.2" extension="PID-50-0001"/>"#, ""),
            Malformation::NoPatientId { event: 0 },
        ),
        (
            base.replace(
                r#"extension="PRPA_IN201306UV02""#,
                r#"extension="PRPA_IN201310UV02""#,
            ),
            Malformation::WrongInteraction,
        ),
        (
            base.replace(
                r#"<queryResponseCode code="OK"/>"#,
                r#"<queryResponseCode code="NF"/>"#,
            ),
            Malformation::MatchesWithoutMatch,
        ),
        (
            fixture("no-match.xml").replace(
                r#"<queryResponseCode code="NF"/>"#,
                r#"<queryResponseCode code="OK"/>"#,
            ),
            Malformation::EmptyMatch,
        ),
        (
            base.replace(r#"<typeCode code="AA"/>"#, r#"<typeCode code="XX"/>"#),
            Malformation::Acknowledgement,
        ),
        (
            base.replace(r#"<queryResponseCode code="OK"/>"#, ""),
            Malformation::QueryResponseCode,
        ),
        (
            base.replace("<env:Body>", "<env:Body><other xmlns=\"urn:example\"/>"),
            Malformation::UnexpectedBody,
        ),
        (
            base.replace(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE env:Envelope>",
            ),
            Malformation::DocumentType,
        ),
        ("<Envelope/>".to_owned(), Malformation::NotAnEnvelope),
    ];
    for (body, expected) in cases {
        let answer = answer_of(200, SOAP_XML, body).await;
        assert_eq!(Some(expected.clone()), malformation(answer), "{expected:?}");
    }
    let broken = answer_of(200, SOAP_XML, base.replace("</env:Body>", "")).await;
    assert!(
        matches!(malformation(broken), Some(Malformation::NotXml { .. })),
        "an answer that is not well formed"
    );
}

#[tokio::test]
async fn a_gateway_silent_past_the_timeout_is_a_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(PATH))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(10)))
        .mount(&server)
        .await;
    let answer = client()
        .discover(
            &responding(&server),
            &query(),
            None,
            Duration::from_millis(200),
        )
        .await;
    assert!(matches!(answer, Err(XcpdError::Timeout)), "{answer:?}");
}
