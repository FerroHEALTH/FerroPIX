// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ID Token of the sign-in signed with ML-DSA-65 (RFC 9964 §5) or ES256,
//! checked by the relying party the console builds, with `[jose]` off and
//! with `post_quantum_only`, which accepts ML-DSA alone.

use std::error::Error;

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
use ferrohealth_smart::jws::algorithm::Algorithm;
use ferrohealth_smart::jws::compact::{Header, encode};
use ferrohealth_smart::jws::key::{PrivateKey, SigningKey};
use ferrohealth_smart::oidc::{Callback, OidcError, PendingSignIn, SignedIn};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{CLIENT_ID, REDIRECT_URI, console};

/// A provider at a mock server publishing the ES256 key `es256` as `es`
/// and the ML-DSA-65 key `ml_dsa` as `ml`.
async fn provider(es256: &SigningKey, ml_dsa: &SigningKey) -> MockServer {
    let server = MockServer::start().await;
    let jwk = |key: &SigningKey, kid: &str| {
        key.public_jwk()
            .clone()
            .with_kid(kid)
            .with_signature_use()
            .to_value()
    };
    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "keys": [jwk(es256, "es"), jwk(ml_dsa, "ml")] })),
        )
        .mount(&server)
        .await;
    server
}

/// The two keys: ES256 on a fresh P-256 key, ML-DSA-65 from a fixed seed.
fn keys() -> Result<(SigningKey, SigningKey), Box<dyn Error>> {
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
            .map_err(|_unspecified| "a P-256 key")?;
    let es256 = SigningKey::new(PrivateKey::from_pkcs8(pkcs8.as_ref())?, Algorithm::Es256)?;
    let ml_dsa = SigningKey::new(
        PrivateKey::from_ml_dsa_seed(Algorithm::MlDsa65, &[13; 32])?,
        Algorithm::MlDsa65,
    )?;
    Ok((es256, ml_dsa))
}

/// Exchanges a code for an ID Token `key` signs under `kid`, at the console
/// whose `[jose]` lines are `jose`.
async fn exchange(
    server: &MockServer,
    key: &SigningKey,
    kid: &str,
    jose: &str,
) -> Result<Result<SignedIn, OidcError>, Box<dyn Error>> {
    let issuer = server.uri();
    let text = format!(
        "[pix]\nbase_url = \"http://127.0.0.1:9/\"\n\n\
         [session]\nsecure_cookie = false\n\n\
         [oidc]\nissuer = \"{issuer}\"\n\
         authorization_endpoint = \"{issuer}/authorize\"\n\
         token_endpoint = \"{issuer}/token\"\n\
         jwks_uri = \"{issuer}/jwks\"\n\
         client_id = \"{CLIENT_ID}\"\n\
         redirect_uri = \"{REDIRECT_URI}\"\n\n{jose}"
    );
    let (state, _service) = console(&text).await?;
    let party = state
        .relying_party()
        .ok_or("sign-in is configured")?
        .clone();
    let pending = PendingSignIn::mint()?;
    let now = jiff::Timestamp::now().as_second();
    let mut header = Header::new(key.algorithm());
    header.kid = Some(kid.to_owned());
    let claims = json!({
        "iss": issuer,
        "sub": "operator-1",
        "aud": CLIENT_ID,
        "iat": now,
        "exp": now + 300,
        "nonce": pending.nonce(),
    });
    let id_token = encode(&header, &claims, key)?;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "synthetic-access-token",
            "token_type": "Bearer",
            "expires_in": 300,
            "id_token": id_token,
        })))
        .mount(server)
        .await;
    Ok(party
        .exchange(&pending, &Callback::granted("c1", pending.state()))
        .await)
}

/// Whether a sign-in with an ID Token signed by the ES256 key (`ml_dsa`
/// false) or the ML-DSA-65 key succeeds at a console with `jose`.
async fn signs_in(ml_dsa: bool, jose: &str) -> Result<bool, Box<dyn Error>> {
    let (es256, ml_dsa_key) = keys()?;
    let server = provider(&es256, &ml_dsa_key).await;
    let (key, kid) = if ml_dsa {
        (&ml_dsa_key, "ml")
    } else {
        (&es256, "es")
    };
    Ok(exchange(&server, key, kid, jose).await?.is_ok())
}

#[tokio::test]
async fn an_ml_dsa_id_token_signs_in_with_or_without_post_quantum_only()
-> Result<(), Box<dyn Error>> {
    assert!(signs_in(true, "").await?, "ML-DSA-65 by default");
    assert!(
        signs_in(true, "[jose]\npost_quantum_only = true\n").await?,
        "ML-DSA-65 under post_quantum_only"
    );
    Ok(())
}

#[tokio::test]
async fn an_es256_id_token_signs_in_by_default_and_is_refused_under_post_quantum_only()
-> Result<(), Box<dyn Error>> {
    assert!(signs_in(false, "").await?, "ES256 by default");
    assert!(
        !signs_in(false, "[jose]\npost_quantum_only = true\n").await?,
        "post_quantum_only refuses ES256"
    );
    Ok(())
}
