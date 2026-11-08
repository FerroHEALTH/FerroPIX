// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The authorizer over `ferrohealth-smart`'s client-credentials token
//! (feature `smart`) against a stub token endpoint and a stub PIX Manager:
//! the bearer token sent (IUA ITI-72 §3.72.4.2), one resend with a new token
//! after a `401` and a second `401` standing (§3.72.4.3), a token that
//! cannot be obtained reported with its cause and nothing sent, and for a
//! grant bound to a `DPoP` key the `DPoP` token sent with a proof of each
//! request (RFC 9449 §7.1) and a resource's nonce answered once with the
//! token kept (§9).

use std::error::Error as _;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ferrohealth_smart::client::cache::TokenSource;
use ferrohealth_smart::client::dpop::{DpopKey, ath};
use ferrohealth_smart::client::{ClientCredentials, Endpoint, TokenError};
use http::StatusCode;
use http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use ihe_atna::user::OnBehalfOf;
use ihe_iti::authorizer::Authorizable;
use ihe_iti::authorizer::smart::TokenAuthorizer;
use ihe_iti::pixm::error::PixmError;
use ihe_iti::pixm::identifier::CrossReference;
use secrecy::SecretString;
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::authorized::{ALL, manager_accepting};
use super::{FHIR_JSON, OPERATION, PROMPT, client, red_source, vendored};

/// The token endpoint's path on the stub authorization server.
const TOKEN: &str = "/token";

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

/// The nonce the stub PIX Manager demands of a `DPoP` proof.
const NONCE: &str = "synthetic-nonce-1";

/// A stub authorization server issuing the `DPoP`-bound `token-1`, then
/// `token-2` for every later request (RFC 9449 §5).
async fn dpop_authorization_server() -> MockServer {
    let server = MockServer::start().await;
    for (token, times, priority) in [("token-1", Some(1), 1), ("token-2", None, 2)] {
        let mock = Mock::given(method("POST")).and(path(TOKEN)).respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": token,
                "token_type": "DPoP",
                "expires_in": 300,
            })),
        );
        let mock = match times {
            Some(times) => mock.up_to_n_times(times),
            None => mock,
        };
        mock.with_priority(priority).mount(&server).await;
    }
    server
}

/// A `DPoP` key generated for the test on P-256 (ES256).
fn dpop_key() -> Arc<DpopKey> {
    let pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).expect("a P-256 key");
    Arc::new(DpopKey::from_pem(pair.serialize_pem().as_bytes()).expect("a DPoP key"))
}

/// The authorizer over a token source of `server`'s token endpoint, its
/// tokens bound to `key`.
fn dpop_authorizer(server: &MockServer, key: Arc<DpopKey>) -> Arc<TokenAuthorizer> {
    let endpoint = Url::parse(&format!("{}{TOKEN}", server.uri())).expect("the token endpoint");
    let config = ClientCredentials::new(
        Endpoint::Token(endpoint),
        String::from("synthetic-client"),
        SecretString::from("synthetic-secret"),
    )
    .with_dpop(key);
    Arc::new(TokenAuthorizer::new(
        TokenSource::new(config).expect("a token source"),
    ))
}

/// The claims of a `DPoP` proof a request is checked by (RFC 9449 §4.2).
#[derive(Debug, serde::Deserialize)]
struct ProofClaims {
    htm: String,
    htu: String,
    ath: String,
    nonce: Option<String>,
}

/// The claims of the `DPoP` proof `request` carries, unverified.
fn proof_claims(request: &Request) -> Option<ProofClaims> {
    let proof = request.headers.get("dpop")?.to_str().ok()?;
    let payload = proof.split('.').nth(1)?;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()
}

/// Whether `request` proves a `GET` of the operation at `base` presenting
/// `token`, with `nonce` when one is named (RFC 9449 §4.2, §7.1).
fn proves(request: &Request, base: &str, token: &str, nonce: Option<&str>) -> bool {
    proof_claims(request).is_some_and(|claims| {
        claims.htm == "GET"
            && claims.htu == format!("{base}{OPERATION}")
            && claims.ath == ath(token)
            && claims.nonce.as_deref() == nonce
    })
}

/// A stub PIX Manager answering a request that presents `DPoP token-1` with
/// a proof of it, carrying [`NONCE`] when `demand_nonce`; a proof without
/// the nonce is answered with the RFC 9449 §9 challenge, and anything else
/// with a plain `401`.
async fn dpop_manager(demand_nonce: bool) -> MockServer {
    let server = MockServer::start().await;
    let base = server.uri();
    let nonce = demand_nonce.then_some(NONCE);
    let accepted = base.clone();
    Mock::given(method("GET"))
        .and(path(OPERATION))
        .and(header(AUTHORIZATION, "DPoP token-1"))
        .and(move |request: &Request| proves(request, &accepted, "token-1", nonce))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(vendored(ALL).into_bytes(), FHIR_JSON),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    if demand_nonce {
        Mock::given(method("GET"))
            .and(path(OPERATION))
            .and(header(AUTHORIZATION, "DPoP token-1"))
            .and(move |request: &Request| proves(request, &base, "token-1", None))
            .respond_with(
                ResponseTemplate::new(401)
                    .insert_header(
                        WWW_AUTHENTICATE,
                        r#"DPoP error="use_dpop_nonce", error_description="Resource server requires nonce in DPoP proof""#,
                    )
                    .insert_header("DPoP-Nonce", NONCE),
            )
            .with_priority(2)
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path(OPERATION))
        .respond_with(ResponseTemplate::new(401))
        .with_priority(3)
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn a_dpop_bound_token_is_sent_with_a_proof_of_the_request() {
    let tokens = dpop_authorization_server().await;
    let manager = dpop_manager(false).await;
    let answer = client(&manager)
        .with_authorizer(dpop_authorizer(&tokens, dpop_key()))
        .cross_reference(&red_source(), &[], &OnBehalfOf::System, PROMPT)
        .await;
    assert!(
        matches!(answer, Ok(CrossReference::Matched(_))),
        "{answer:?}"
    );
    assert_eq!(vec![String::from("DPoP token-1")], sent(&manager).await);
    assert_eq!(1, token_requests(&tokens).await);
}

#[tokio::test]
async fn a_resources_dpop_nonce_is_answered_once_with_the_token_kept() {
    let tokens = dpop_authorization_server().await;
    let manager = dpop_manager(true).await;
    let answer = client(&manager)
        .with_authorizer(dpop_authorizer(&tokens, dpop_key()))
        .cross_reference(&red_source(), &[], &OnBehalfOf::System, PROMPT)
        .await;
    assert!(
        matches!(answer, Ok(CrossReference::Matched(_))),
        "{answer:?}"
    );
    assert_eq!(
        vec![String::from("DPoP token-1"), String::from("DPoP token-1")],
        sent(&manager).await,
        "the nonce refuses the proof, never the token"
    );
    assert_eq!(1, token_requests(&tokens).await, "no new token is fetched");
}

#[tokio::test]
async fn a_refused_dpop_bound_token_is_dropped_and_the_request_resent_once_with_a_new_one() {
    let tokens = dpop_authorization_server().await;
    let manager = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(OPERATION))
        .and(header(AUTHORIZATION, "DPoP token-2"))
        .and(|request: &Request| request.headers.contains_key("dpop"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(vendored(ALL).into_bytes(), FHIR_JSON),
        )
        .with_priority(1)
        .mount(&manager)
        .await;
    Mock::given(method("GET"))
        .and(path(OPERATION))
        .respond_with(
            ResponseTemplate::new(401)
                .insert_header(WWW_AUTHENTICATE, r#"DPoP error="invalid_token""#),
        )
        .with_priority(2)
        .mount(&manager)
        .await;
    let answer = client(&manager)
        .with_authorizer(dpop_authorizer(&tokens, dpop_key()))
        .cross_reference(&red_source(), &[], &OnBehalfOf::System, PROMPT)
        .await;
    assert!(
        matches!(answer, Ok(CrossReference::Matched(_))),
        "{answer:?}"
    );
    assert_eq!(
        vec![String::from("DPoP token-1"), String::from("DPoP token-2")],
        sent(&manager).await
    );
    assert_eq!(2, token_requests(&tokens).await);
}
