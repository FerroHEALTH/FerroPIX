// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The credential of the endpoint table the Localization Service client is
//! built from: every request carries a `DPoP`-bound token with a proof made
//! for that request (the IG's GFI-005; RFC 9449 §7.1) whose `htu` holds no
//! query, so the pseudonym never reaches it (§4.2), a request no token can
//! be had for is never sent, a resource's nonce is answered once with the
//! token kept (§9), and a request is sent at most twice.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ferrohealth_secret::secret_url::SecretUrl;
use ferrohealth_smart::client::dpop::ath;
use ferrohealth_smart::config::credentials::{CredentialsSection, OauthSection};
use http::StatusCode;
use http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use nl_generic_functions::nvi::NviClient;
use nl_generic_functions::nvi::error::NviError;
use secrecy::SecretString;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::{FHIR_JSON, PATIENT, PROMPT, SEARCH, patient, record, searchset};
use crate::outbound::{build, endpoint};

/// The token endpoint's path on the stub authorization server.
const TOKEN: &str = "/token";

/// The nonce the stub service demands of a proof.
const NONCE: &str = "synthetic-nonce-1";

/// A stub authorization server issuing the `DPoP`-bound `token-1`.
async fn authorization_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "token-1",
            "token_type": "DPoP",
            "expires_in": 300,
        })))
        .mount(&server)
        .await;
    server
}

/// A `DPoP` key generated for the test on P-256 (ES256), in a PEM file
/// under `directory`.
fn dpop_key(directory: &Path) -> PathBuf {
    let pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).expect("a P-256 key");
    let file = directory.join("dpop.pem");
    std::fs::write(&file, pair.serialize_pem()).expect("the key file");
    file
}

/// A client of `service` whose endpoint table names the client credentials
/// grant of `tokens`, its tokens bound to the key in `key`.
fn client(service: &MockServer, tokens: &MockServer, key: &Path) -> NviClient {
    let mut table = endpoint(&format!("{}/fhir", service.uri()));
    table.credentials = CredentialsSection {
        oauth: Some(OauthSection {
            token_endpoint: SecretUrl::new(format!("{}{TOKEN}", tokens.uri())),
            client_id: String::from("synthetic-client"),
            client_secret: Some(SecretString::from("synthetic-secret")),
            dpop_key_file: Some(key.to_path_buf()),
            ..OauthSection::default()
        }),
        ..CredentialsSection::default()
    };
    NviClient::new(build(&table)).expect("a client")
}

/// The claims of a proof a request is checked by (RFC 9449 §4.2).
#[derive(Debug, serde::Deserialize)]
struct ProofClaims {
    htm: String,
    htu: String,
    ath: String,
    nonce: Option<String>,
}

/// The claims of the proof `request` carries, unverified.
fn proof_claims(request: &Request) -> Option<ProofClaims> {
    let proof = request.headers.get("dpop")?.to_str().ok()?;
    let payload = proof.split('.').nth(1)?;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()
}

/// Whether `request` proves a `GET` of the search at `base` presenting
/// `token-1`, with `nonce` when one is named.
fn proves(request: &Request, base: &str, nonce: Option<&str>) -> bool {
    proof_claims(request).is_some_and(|claims| {
        claims.htm == "GET"
            && claims.htu == format!("{base}{SEARCH}")
            && claims.ath == ath("token-1")
            && claims.nonce.as_deref() == nonce
    })
}

/// A service answering a search that presents `DPoP token-1` with a proof
/// of it with one record, carrying [`NONCE`] when `demand_nonce`; a proof
/// without the nonce is answered with the §9 challenge, and anything else
/// with a plain `401`.
async fn proven_service(demand_nonce: bool) -> MockServer {
    let server = MockServer::start().await;
    let base = server.uri();
    let nonce = demand_nonce.then_some(NONCE);
    let accepted = base.clone();
    let body = searchset(vec![record(PATIENT, "ura-test-0001")], None);
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .and(header(AUTHORIZATION, "DPoP token-1"))
        .and(move |request: &Request| proves(request, &accepted, nonce))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(body.to_string().into_bytes(), FHIR_JSON),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    if demand_nonce {
        Mock::given(method("GET"))
            .and(path(SEARCH))
            .and(move |request: &Request| proves(request, &base, None))
            .respond_with(
                ResponseTemplate::new(401)
                    .insert_header(WWW_AUTHENTICATE, r#"DPoP error="use_dpop_nonce""#)
                    .insert_header("DPoP-Nonce", NONCE),
            )
            .with_priority(2)
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .respond_with(ResponseTemplate::new(401))
        .with_priority(3)
        .mount(&server)
        .await;
    server
}

/// The requests `server` received.
async fn requests(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.unwrap_or_default()
}

#[tokio::test]
async fn every_request_carries_the_token_and_a_proof_made_for_it() {
    let keys = tempfile::tempdir().expect("a key directory");
    let tokens = authorization_server().await;
    let service = proven_service(false).await;
    let localization = client(&service, &tokens, &dpop_key(keys.path()))
        .localize(&patient(), PROMPT)
        .await
        .expect("a localization");
    assert_eq!(1, localization.custodians().len());
    assert_eq!(1, requests(&service).await.len());
}

#[tokio::test]
async fn neither_the_proof_nor_the_token_request_names_the_pseudonym() {
    let keys = tempfile::tempdir().expect("a key directory");
    let tokens = authorization_server().await;
    let service = proven_service(false).await;
    client(&service, &tokens, &dpop_key(keys.path()))
        .localize(&patient(), PROMPT)
        .await
        .expect("a localization");
    for request in requests(&service).await {
        let claims = proof_claims(&request).expect("a proof");
        assert!(!claims.htu.contains(PATIENT), "the htu of RFC 9449 §4.2");
        assert!(!claims.htu.contains('?'), "the htu holds no query");
    }
    for request in requests(&tokens).await {
        assert!(
            !String::from_utf8_lossy(&request.body).contains(PATIENT),
            "the token request names the pseudonym"
        );
    }
}

#[tokio::test]
async fn a_request_no_token_can_be_had_for_is_never_sent() {
    let keys = tempfile::tempdir().expect("a key directory");
    let tokens = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(TOKEN))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": "invalid_client",
        })))
        .mount(&tokens)
        .await;
    let service = proven_service(false).await;
    let error = client(&service, &tokens, &dpop_key(keys.path()))
        .localize(&patient(), PROMPT)
        .await
        .expect_err("no credential");
    assert!(matches!(error, NviError::Unauthenticated(_)), "{error:?}");
    assert!(requests(&service).await.is_empty(), "nothing was sent");
}

#[tokio::test]
async fn a_resources_nonce_is_answered_once_with_the_token_kept() {
    let keys = tempfile::tempdir().expect("a key directory");
    let tokens = authorization_server().await;
    let service = proven_service(true).await;
    let localization = client(&service, &tokens, &dpop_key(keys.path()))
        .localize(&patient(), PROMPT)
        .await
        .expect("a localization on the resend");
    assert_eq!(1, localization.custodians().len());
    assert_eq!(2, requests(&service).await.len());
    assert_eq!(1, requests(&tokens).await.len(), "no new token is fetched");
}

#[tokio::test]
async fn a_request_is_sent_at_most_twice() {
    let keys = tempfile::tempdir().expect("a key directory");
    let tokens = authorization_server().await;
    let service = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .respond_with(ResponseTemplate::new(401))
        .mount(&service)
        .await;
    let error = client(&service, &tokens, &dpop_key(keys.path()))
        .localize(&patient(), PROMPT)
        .await
        .expect_err("refused twice");
    assert!(
        matches!(error, NviError::Rejected { status } if status == StatusCode::UNAUTHORIZED),
        "{error:?}"
    );
    assert_eq!(2, requests(&service).await.len(), "one resend at most");
}

#[tokio::test]
async fn the_client_renders_whether_it_carries_a_credential_and_never_the_credential() {
    let keys = tempfile::tempdir().expect("a key directory");
    let tokens = authorization_server().await;
    let service = proven_service(false).await;
    let rendered = format!("{:?}", client(&service, &tokens, &dpop_key(keys.path())));
    assert!(rendered.contains("authorization: true"), "{rendered}");
    assert!(!rendered.contains("synthetic-secret"), "{rendered}");
}
