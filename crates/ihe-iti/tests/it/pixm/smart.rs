// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The authorizer over `ferrohealth-smart`'s client-credentials token
//! (feature `smart`) against a stub token endpoint and a stub PIX Manager:
//! the bearer token sent (IUA ITI-72 §3.72.4.2), one resend with a new token
//! after a `401` and a second `401` standing (§3.72.4.3), and a token that
//! cannot be obtained reported with its cause and nothing sent.

use std::error::Error as _;
use std::sync::Arc;

use ferrohealth_smart::client::cache::TokenSource;
use ferrohealth_smart::client::{ClientCredentials, Endpoint, TokenError};
use http::StatusCode;
use http::header::AUTHORIZATION;
use ihe_atna::user::OnBehalfOf;
use ihe_iti::authorizer::smart::TokenAuthorizer;
use ihe_iti::pixm::error::PixmError;
use ihe_iti::pixm::identifier::CrossReference;
use secrecy::SecretString;
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{FHIR_JSON, OPERATION, PROMPT, client, red_source, vendored};

/// The token endpoint's path on the stub authorization server.
const TOKEN: &str = "/token";

/// The IG's answer for the red patient in every domain.
const ALL: &str = "example/Parameters-pixm-response-mohralice-red-all.json";

/// A token response issuing `token` (RFC 6749 §5.1).
fn issued(token: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "access_token": token,
        "token_type": "Bearer",
        "expires_in": 300,
    }))
}

/// A stub authorization server issuing `token-1`, then `token-2` for every
/// later request.
async fn authorization_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN))
        .respond_with(issued("token-1"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(TOKEN))
        .respond_with(issued("token-2"))
        .with_priority(2)
        .mount(&server)
        .await;
    server
}

/// The authorizer over a token source of `server`'s token endpoint.
fn authorizer(server: &MockServer) -> Arc<TokenAuthorizer> {
    let endpoint = Url::parse(&format!("{}{TOKEN}", server.uri())).expect("the token endpoint");
    let config = ClientCredentials::new(
        Endpoint::Token(endpoint),
        String::from("synthetic-client"),
        SecretString::from("synthetic-secret"),
    );
    Arc::new(TokenAuthorizer::new(
        TokenSource::new(config).expect("a token source"),
    ))
}

/// A stub Manager that answers a request carrying `accepted` with the IG's
/// answer, and every other with `401`.
async fn manager_accepting(accepted: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(OPERATION))
        .and(header(AUTHORIZATION, accepted))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(vendored(ALL).into_bytes(), FHIR_JSON),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(OPERATION))
        .respond_with(ResponseTemplate::new(401))
        .with_priority(2)
        .mount(&server)
        .await;
    server
}

/// The `Authorization` values `server` received, in order.
async fn sent(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|request| request.headers.get(AUTHORIZATION))
        .filter_map(|value| value.to_str().ok().map(str::to_owned))
        .collect()
}

/// The number of token requests `server` received.
async fn token_requests(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .map_or(0, |requests| requests.len())
}

#[tokio::test]
async fn the_sources_token_is_sent_as_a_bearer_token() {
    let tokens = authorization_server().await;
    let manager = manager_accepting("Bearer token-1").await;
    let answer = client(&manager)
        .with_authorizer(authorizer(&tokens))
        .cross_reference(&red_source(), &[], &OnBehalfOf::System, PROMPT)
        .await;
    assert!(
        matches!(answer, Ok(CrossReference::Matched(_))),
        "{answer:?}"
    );
    assert_eq!(vec![String::from("Bearer token-1")], sent(&manager).await);
    assert_eq!(1, token_requests(&tokens).await);
}

#[tokio::test]
async fn a_refused_token_is_dropped_and_the_request_resent_once_with_a_new_one() {
    let tokens = authorization_server().await;
    let manager = manager_accepting("Bearer token-2").await;
    let answer = client(&manager)
        .with_authorizer(authorizer(&tokens))
        .cross_reference(&red_source(), &[], &OnBehalfOf::System, PROMPT)
        .await;
    assert!(
        matches!(answer, Ok(CrossReference::Matched(_))),
        "{answer:?}"
    );
    assert_eq!(
        vec![
            String::from("Bearer token-1"),
            String::from("Bearer token-2")
        ],
        sent(&manager).await,
        "the refused token is never sent again"
    );
    assert_eq!(2, token_requests(&tokens).await);
}

#[tokio::test]
async fn a_second_401_stands() {
    let tokens = authorization_server().await;
    let manager = manager_accepting("Bearer never-issued").await;
    let answer = client(&manager)
        .with_authorizer(authorizer(&tokens))
        .cross_reference(&red_source(), &[], &OnBehalfOf::System, PROMPT)
        .await;
    assert!(
        matches!(
            answer,
            Err(PixmError::Rejected {
                status: StatusCode::UNAUTHORIZED,
                ..
            })
        ),
        "{answer:?}"
    );
    assert_eq!(2, sent(&manager).await.len(), "one resend, no more");
}

#[tokio::test]
async fn a_token_that_cannot_be_obtained_is_an_error_with_its_cause_and_nothing_is_sent() {
    let tokens = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "invalid_client",
        })))
        .mount(&tokens)
        .await;
    let manager = manager_accepting("Bearer token-1").await;
    let answer = client(&manager)
        .with_authorizer(authorizer(&tokens))
        .cross_reference(&red_source(), &[], &OnBehalfOf::System, PROMPT)
        .await;
    let Err(PixmError::Unauthenticated(error)) = answer else {
        panic!("no token, no request: {answer:?}");
    };
    let cause = error.source().expect("the token failure is the source");
    assert!(
        cause.downcast_ref::<Arc<TokenError>>().is_some(),
        "the source is the TokenError: {cause}"
    );
    assert_eq!(Vec::<String>::new(), sent(&manager).await);
}
