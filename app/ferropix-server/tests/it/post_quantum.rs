// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Access tokens signed with ML-DSA-65 (RFC 9964 §5) through the guard,
//! under the issuer's default algorithms and under `[auth]
//! post_quantum_only`.

use http::StatusCode;

use crate::auth::{
    EVERY_SCOPE, Issuer, application, bearer, guarded, handled, send, settings_with, uri_of,
};
use crate::support::Secrets;

/// The ML-DSA-65 key of the post-quantum cases (RFC 9964 §5), from a fixed
/// seed.
pub(crate) fn ml_dsa_key() -> ferrohealth_smart::jws::key::SigningKey {
    let algorithm = ferrohealth_smart::jws::algorithm::Algorithm::MlDsa65;
    let private = ferrohealth_smart::jws::key::PrivateKey::from_ml_dsa_seed(algorithm, &[3; 32])
        .expect("a 32-octet seed");
    ferrohealth_smart::jws::key::SigningKey::new(private, algorithm)
        .expect("an ML-DSA-65 key signs ML-DSA-65")
}

/// The claims of the test issuer's access token (RFC 9068 §2.2).
#[derive(serde::Deserialize, serde::Serialize)]
struct Claims {
    /// The issuer.
    iss: String,
    /// The audience.
    aud: String,
    /// The subject.
    sub: String,
    /// The client.
    client_id: String,
    /// The issue time.
    iat: i64,
    /// The expiry.
    exp: i64,
    /// The token's id.
    jti: String,
    /// The granted scope.
    scope: String,
}

/// The claims of the ES256 `token`, signed again with the ML-DSA-65 key as
/// `ml`.
fn ml_dsa_token(token: &str) -> String {
    let payload = token.split('.').nth(1).expect("a JWS");
    let claims: Claims = serde_json::from_slice(
        &base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, payload)
            .expect("base64url"),
    )
    .expect("the claims");
    let mut header = ferrohealth_smart::jws::compact::Header::new(
        ferrohealth_smart::jws::algorithm::Algorithm::MlDsa65,
    );
    header.kid = Some(String::from("ml"));
    header.typ = Some(String::from("at+jwt"));
    ferrohealth_smart::jws::compact::encode(&header, &claims, &ml_dsa_key())
        .expect("the token signs")
}

// RFC 9964 §5: under `[auth] post_quantum_only` an issuer's default list is
// the ML-DSA parameter sets, so an ML-DSA-65 token is admitted and an ES256
// one refused; off, the default list is classical.
#[tokio::test]
async fn auth_post_quantum_only_admits_an_ml_dsa_token_and_refuses_an_es256_one() {
    let issuer = Issuer::start_with_ml_dsa().await;
    let secrets = Secrets::new();
    let route = guarded()
        .find(|route| route.interaction != "operator-dependencies")
        .expect("a guarded route");
    for on in [false, true] {
        let extra = if on {
            "[auth]\npost_quantum_only = true\n"
        } else {
            ""
        };
        let (app, _, _spool) = application(settings_with(&issuer, &secrets, extra)).await;
        let classical = issuer.token("east-lab", EVERY_SCOPE);
        let quantum = ml_dsa_token(&classical);
        let es256 = send(
            &app,
            route.method.clone(),
            &uri_of(route),
            Some(&bearer(&classical)),
        )
        .await;
        let ml_dsa = send(
            &app,
            route.method.clone(),
            &uri_of(route),
            Some(&bearer(&quantum)),
        )
        .await;
        if on {
            assert_eq!(
                StatusCode::UNAUTHORIZED,
                es256.status,
                "ES256 under the switch"
            );
            assert!(
                handled(ml_dsa.status),
                "ML-DSA-65 under the switch: {}",
                ml_dsa.status
            );
        } else {
            assert!(handled(es256.status), "ES256 by default: {}", es256.status);
            assert_eq!(
                StatusCode::UNAUTHORIZED,
                ml_dsa.status,
                "ML-DSA-65 by default"
            );
        }
    }
}
