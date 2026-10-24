// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The container cases. Each returns at once without `FERROPIX_E2E=1`.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ferropix_testkit::containers::{e2e_enabled, postgres, stub_provider};
use serde::Deserialize;

#[tokio::test]
async fn postgres_starts_at_major_version_18() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let pool = sqlx::PgPool::connect(postgres.url()).await.unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        version.starts_with("18"),
        "the server is PostgreSQL 18, got {version}"
    );
    pool.close().await;
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    token_endpoint: String,
    jwks_uri: String,
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: Audience,
    scope: String,
}

/// The `aud` claim, one string or an array of them (RFC 7519 §4.1.3).
#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    fn names(&self, audience: &str) -> bool {
        match self {
            Self::One(one) => one == audience,
            Self::Many(many) => many.iter().any(|name| name == audience),
        }
    }
}

#[tokio::test]
async fn the_stub_provider_signs_a_client_credentials_token() {
    if !e2e_enabled() {
        return;
    }
    let provider = stub_provider("ferropix-e2e").await.unwrap();
    let discovery: Discovery = reqwest::get(provider.discovery_url())
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap();
    assert_eq!(
        provider.issuer(),
        discovery.issuer,
        "the discovery document names the issuer"
    );
    assert_eq!(
        provider.token_url(),
        discovery.token_endpoint,
        "the token endpoint"
    );
    assert!(
        discovery.jwks_uri.starts_with(&provider.issuer()),
        "the key set is the issuer's"
    );

    let token = provider
        .client_credentials("synthetic-source", "system/Patient.rs")
        .await
        .unwrap();
    let claims = decode_segment(token.split('.').nth(1).unwrap());
    assert_eq!(provider.issuer(), claims.iss, "iss");
    assert_eq!("synthetic-source", claims.sub, "sub");
    assert!(claims.aud.names("ferropix-e2e"), "aud names the audience");
    assert_eq!("system/Patient.rs", claims.scope, "scope");
}

/// Decodes the base64url payload segment of a JWT (RFC 7515 §2) into its
/// claims.
fn decode_segment(segment: &str) -> Claims {
    let bytes = URL_SAFE_NO_PAD.decode(segment).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
