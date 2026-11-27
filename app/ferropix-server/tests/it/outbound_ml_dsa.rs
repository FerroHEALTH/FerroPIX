// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The client assertion FerroPIX signs for a client credentials grant
//! (`private_key_jwt`, RFC 7523 §2.2) under an ML-DSA-65 key: signed with
//! ML-DSA-65 (RFC 9964 §5) and verified against the key's public half. The
//! outbound client is the one the audit repository and the terminology
//! server are reached through, `ferrohealth-smart`'s `OutboundClient`.

use aws_lc_rs::signature::{ML_DSA_65_SIGNING, PqdsaKeyPair};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ferrohealth_smart::client::ClientAuth;
use ferrohealth_smart::client::outbound::OutboundClient;
use ferrohealth_smart::config::ServerTls;
use ferrohealth_smart::config::credentials::{CredentialsSection, OauthSection};
use ferrohealth_smart::config::endpoint::EndpointSection;
use ferrohealth_smart::jws::algorithm::Algorithm;
use ferrohealth_smart::jws::compact::decode;
use ferrohealth_smart::jws::key::PrivateKey;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn an_ml_dsa_key_signs_the_client_assertion_with_ml_dsa_65() {
    let pkcs8 = PqdsaKeyPair::generate(&ML_DSA_65_SIGNING)
        .expect("an ML-DSA-65 key pair")
        .to_pkcs8v1()
        .expect("its PKCS#8 form");
    let public = PrivateKey::from_pkcs8(pkcs8.as_ref())
        .expect("the key reads")
        .public_jwk()
        .clone();
    let directory = tempfile::tempdir().expect("a temporary directory");
    let key = directory.path().join("assertion.pem");
    let body = STANDARD.encode(pkcs8.as_ref());
    let lines: Vec<String> = body
        .as_bytes()
        .chunks(64)
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect();
    std::fs::write(
        &key,
        format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            lines.join("\n")
        ),
    )
    .expect("the key file writes");

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "synthetic-access",
            "token_type": "Bearer",
            "expires_in": 300
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fhir/metadata"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let endpoint = EndpointSection {
        url: ferrohealth_secret::secret_url::SecretUrl::new(format!("{}/fhir", server.uri())),
        credentials: CredentialsSection {
            oauth: Some(OauthSection {
                token_endpoint: ferrohealth_secret::secret_url::SecretUrl::new(format!(
                    "{}/token",
                    server.uri()
                )),
                client_id: String::from("ferropix"),
                client_auth: ClientAuth::PrivateKeyJwt,
                private_key_file: Some(key),
                ..OauthSection::default()
            }),
            ..CredentialsSection::default()
        },
        ..EndpointSection::default()
    };
    let client = OutboundClient::build(&endpoint, "terminology.endpoint", &ServerTls)
        .expect("the client builds");
    let url = url::Url::parse(&format!("{}/fhir/metadata", server.uri())).expect("a URL");
    let answer = client
        .send(&http::Method::GET, &url, |request| request)
        .await
        .expect("the call is authorized and answered");
    assert_eq!(http::StatusCode::OK, answer.status());

    let requests = server.received_requests().await.expect("recorded requests");
    let grant = requests
        .iter()
        .find(|request| request.url.path() == "/token")
        .expect("one token request");
    let assertion = url::form_urlencoded::parse(&grant.body)
        .find(|(name, _)| name == "client_assertion")
        .map(|(_, value)| value.into_owned())
        .expect("a client assertion");
    let token = decode(&assertion).expect("a compact JWS");
    assert_eq!(Algorithm::MlDsa65, token.header().alg, "RFC 9964 §5");
    let payload = token
        .verify(&public)
        .expect("the assertion verifies against the key's public half");
    let claims: Issued = serde_json::from_slice(payload).expect("the assertion's claims");
    assert_eq!("ferropix", claims.iss);
}

/// The claim of the assertion the test reads.
#[derive(serde::Deserialize)]
struct Issued {
    /// The client, its issuer (RFC 7523 §3).
    iss: String,
}
