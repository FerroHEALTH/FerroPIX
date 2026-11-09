// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The capabilities interaction against a stub PIX Manager (FHIR R4 http.html
//! §capabilities): `GET [base]/metadata` through the client's transport and
//! authorizer, and every outcome a typed answer.

use std::sync::Arc;

use http::header::{ACCEPT, AUTHORIZATION};
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use ihe_iti::authorizer::{Authorizable, Authorized, Authorizer, Retry};
use ihe_iti::pixm::capabilities::{CapabilitiesError, NotACapabilityStatement};
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{FHIR_JSON, PROMPT, client, unreachable_client};

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

/// An authorizer that hands out one fixed token.
#[derive(Debug)]
struct Fixed;

impl Authorizer for Fixed {
    fn authorize<'a>(&'a self, _method: &'a Method, _url: &'a Url) -> Authorized<'a> {
        Box::pin(async move {
            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer token-1"));
            Ok(headers)
        })
    }

    fn answered(&self, _url: &Url, _status: StatusCode, _headers: &HeaderMap) -> Retry {
        Retry::Done
    }
}

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
async fn the_request_carries_the_authorizers_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(METADATA))
        .and(header(AUTHORIZATION, "Bearer token-1"))
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
    let answer = client(&server)
        .with_authorizer(Arc::new(Fixed))
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
