// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The capabilities interaction against a stub PIX Manager (FHIR R4 http.html
//! §capabilities): `GET [base]/metadata` through the client's transport and
//! credential, and every outcome a typed answer.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ferrohealth_smart::config::credentials::CredentialsSection;
use http::StatusCode;
use http::header::{ACCEPT, AUTHORIZATION};
use ihe_iti::pixm::PixmClient;
use ihe_iti::pixm::capabilities::{CapabilitiesError, NotACapabilityStatement};
use secrecy::SecretString;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{BASE, FHIR_JSON, PROMPT, client, unreachable_client};
use crate::outbound::{build, endpoint};

/// The capabilities path under the stub's FHIR base.
const METADATA: &str = "/fhir/metadata";

/// A minimal FHIR R4 `CapabilityStatement` of a server.
const STATEMENT: &str = r#"{
    "resourceType": "CapabilityStatement",
    "status": "active",
    "date": "2026-11-08",
    "kind": "instance",
    "fhirVersion": "4.0.1",
    "format": ["application/fhir+json"],
    "implementation": {"description": "a synthetic PIX Manager"}
}"#;

/// A stub Manager that answers `GET [base]/metadata` with `status`, the media
/// type `media` and `body`.
async fn manager(status: u16, media: &str, body: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(METADATA))
        .and(header(ACCEPT, FHIR_JSON))
        .respond_with(ResponseTemplate::new(status).set_body_raw(body.as_bytes().to_vec(), media))
        .mount(&server)
        .await;
    server
}

/// The synthetic HTTP Basic credential of the endpoint table.
const USER: &str = "synthetic-user";
const PASSWORD: &str = "synthetic-password";

#[tokio::test]
async fn the_capability_statement_is_read_from_metadata() {
    let server = manager(200, FHIR_JSON, STATEMENT).await;
    let statement = client(&server)
        .capabilities(PROMPT)
        .await
        .expect("a statement");
    assert_eq!(statement.fhir_version.value.as_deref(), Some("4.0.1"));
}

#[tokio::test]
async fn the_request_carries_the_tables_credential() {
    let server = MockServer::start().await;
    let basic = format!("Basic {}", STANDARD.encode(format!("{USER}:{PASSWORD}")));
    Mock::given(method("GET"))
        .and(path(METADATA))
        .and(header(AUTHORIZATION, basic.as_str()))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(STATEMENT.as_bytes().to_vec(), FHIR_JSON),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(METADATA))
        .respond_with(ResponseTemplate::new(401))
        .with_priority(2)
        .mount(&server)
        .await;
    let mut table = endpoint(&format!("{}{BASE}", server.uri()));
    table.credentials = CredentialsSection {
        user: String::from(USER),
        password: Some(SecretString::from(PASSWORD)),
        ..CredentialsSection::default()
    };
    let answer = PixmClient::new(build(&table))
        .expect("a client")
        .capabilities(PROMPT)
        .await;
    assert!(answer.is_ok(), "{answer:?}");
}

#[tokio::test]
async fn a_refusal_keeps_its_status() {
    let server = manager(503, "text/plain", "maintenance").await;
    match client(&server).capabilities(PROMPT).await {
        Err(CapabilitiesError::Refused { status, issues }) => {
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert!(issues.is_empty(), "{issues:?}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn an_answer_that_is_no_capability_statement_is_no_fhir_server() {
    let server = manager(200, "text/html", "<html><body>a web server</body></html>").await;
    assert!(matches!(
        client(&server).capabilities(PROMPT).await,
        Err(CapabilitiesError::NotAFhirServer(
            NotACapabilityStatement::Unreadable(_)
        ))
    ));
    let server = manager(
        200,
        FHIR_JSON,
        r#"{"resourceType": "Bundle", "type": "searchset"}"#,
    )
    .await;
    assert!(matches!(
        client(&server).capabilities(PROMPT).await,
        Err(CapabilitiesError::NotAFhirServer(
            NotACapabilityStatement::OtherResource
        ))
    ));
}

#[tokio::test]
async fn an_unreachable_manager_is_a_transport_failure() {
    assert!(matches!(
        unreachable_client().capabilities(PROMPT).await,
        Err(CapabilitiesError::Transport(_))
    ));
}
