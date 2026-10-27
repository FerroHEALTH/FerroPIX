// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[auth]` keys of `ferrohealth-smart` the server honours beyond the
//! issuer list: `ca_file`, the trust roots an issuer behind a private
//! certificate authority is read with, and `mode = "introspection"`, the
//! RFC 7662 check of an opaque token. Every key and certificate is generated
//! when the case runs.

use std::path::Path;

use axum::Router;
use axum::routing::get;
use ferrohealth_health::lifecycle::Lifecycle;
use ferrohealth_server::section::{ServerSection, TlsSection};
use ferropix_server::auth::{Smart, SmartError};
use ferropix_server::config::Config;
use ferropix_server::config::settings::Settings;
use http::{Method, StatusCode};
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair};
use wiremock::matchers;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::auth::{AUDIENCE, application, bearer, send, signing_key};
use crate::support::Secrets;

/// The steward scope the operator's dependency report takes.
const STEWARD: &str = "__ferropix.steward";

/// Reads `auth` as the `[auth]` table of a server with the operator API on,
/// under the secrets of `secrets`.
fn settings(auth: &str, secrets: &Secrets) -> Settings {
    let text = format!(
        "public_base_url = \"http://localhost:8080\"\n\n[server]\nlisten = \"127.0.0.1:0\"\n\n{auth}\n\n\
         [operator]\nenabled = true\n"
    );
    Config::from_sources(Some(&text), &secrets.environment())
        .expect("the configuration reads")
        .resolve()
        .expect("the configuration resolves")
}

/// An authority, and the certificate and key it signs for `localhost`, as
/// PEM files under `directory`: `(authority, certificate, key)`.
fn pki(directory: &Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let mut params = CertificateParams::new(Vec::<String>::new()).expect("authority params");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, "Synthetic private authority");
    let authority = CertifiedIssuer::self_signed(params, KeyPair::generate().expect("a key"))
        .expect("the authority signs itself");
    let mut leaf = CertificateParams::new(vec![String::from("localhost")]).expect("leaf params");
    leaf.distinguished_name
        .push(DnType::CommonName, "Synthetic authorization server");
    let key = KeyPair::generate().expect("a key");
    let certificate = leaf.signed_by(&key, &authority).expect("signed");
    let paths = (
        directory.join("authority.pem"),
        directory.join("server.pem"),
        directory.join("server.key"),
    );
    std::fs::write(&paths.0, authority.pem()).expect("the authority is written");
    std::fs::write(&paths.1, certificate.pem()).expect("the certificate is written");
    std::fs::write(&paths.2, key.serialize_pem()).expect("the key is written");
    paths
}

#[tokio::test(flavor = "multi_thread")]
async fn an_issuer_behind_a_private_authority_is_read_with_the_ca_file_roots_and_not_without() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let (authority, certificate, key_file) = pki(directory.path());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let issuer = format!(
        "https://localhost:{}",
        listener.local_addr().expect("an address").port()
    );
    let key = signing_key();
    let metadata = serde_json::json!({
        "issuer": issuer,
        "jwks_uri": format!("{issuer}/jwks"),
        "token_endpoint": format!("{issuer}/token"),
        "grant_types_supported": ["client_credentials"],
    });
    let jwks = serde_json::json!({ "keys": [key.jwk.clone()] });
    let routes = Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(move || async move { axum::Json(metadata.clone()) }),
        )
        .route(
            "/jwks",
            get(move || async move { axum::Json(jwks.clone()) }),
        );
    let section = ServerSection {
        tls: Some(TlsSection::new(certificate, key_file)),
        ..ServerSection::default()
    };
    tokio::spawn(async move {
        ferrohealth_server::serve::run(
            listener,
            routes,
            &section,
            Lifecycle::default(),
            std::future::pending(),
        )
        .await
    });
    let secrets = Secrets::new();
    let table =
        |ca: &str| format!("[auth]\nissuer = \"{issuer}\"\naudience = [\"{AUDIENCE}\"]\n{ca}");
    let untrusted = settings(&table(""), &secrets);
    let refused = Smart::discover(untrusted.auth, &untrusted.domains)
        .await
        .expect_err("the platform's roots do not hold the private authority");
    assert!(matches!(refused, SmartError::Issuer { .. }), "{refused:?}");
    let trusted = settings(
        &table(&format!("ca_file = \"{}\"\n", authority.display())),
        &secrets,
    );
    let (app, smart, _spool) = application(trusted).await;
    assert_eq!(
        Some(format!("{issuer}/token").as_str()),
        smart.token_endpoint()
    );
    let token = bearer(&key.token(&issuer, "console", STEWARD));
    let answer = send(&app, Method::GET, "/operator/dependencies", Some(&token)).await;
    assert_eq!(StatusCode::OK, answer.status, "{}", answer.body);
}

#[tokio::test(flavor = "multi_thread")]
async fn under_introspection_an_opaque_token_is_checked_at_the_configured_endpoint() {
    let server = MockServer::start().await;
    let uri = server.uri();
    Mock::given(matchers::method("GET"))
        .and(matchers::path("/.well-known/oauth-authorization-server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issuer": uri,
            "token_endpoint": format!("{uri}/token"),
            "introspection_endpoint": format!("{uri}/metadata-introspect"),
            "grant_types_supported": ["client_credentials"],
        })))
        .mount(&server)
        .await;
    let now = jiff::Timestamp::now().as_second();
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/introspect"))
        .and(matchers::body_string_contains(
            "token=opaque-synthetic-token",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "active": true,
            "iss": uri,
            "aud": AUDIENCE,
            "client_id": "console",
            "sub": "synthetic-subject-17",
            "scope": STEWARD,
            "token_type": "Bearer",
            "exp": now + 300,
        })))
        .expect(1)
        .mount(&server)
        .await;
    let secrets = Secrets::new();
    let settings = settings(
        &format!(
            "[auth]\nissuer = \"{uri}\"\naudience = [\"{AUDIENCE}\"]\nmode = \"introspection\"\n\n\
             [auth.introspection]\nendpoint = \"{uri}/introspect\"\n\n\
             [auth.introspection.client]\nclient_id = \"ferropix\"\nclient_secret = \"synthetic-secret\"\n"
        ),
        &secrets,
    );
    let (app, smart, _spool) = application(settings).await;
    assert_eq!(
        Some(format!("{uri}/token").as_str()),
        smart.token_endpoint()
    );
    let answer = send(
        &app,
        Method::GET,
        "/operator/dependencies",
        Some(&bearer("opaque-synthetic-token")),
    )
    .await;
    assert_eq!(StatusCode::OK, answer.status, "{}", answer.body);
    let refused = send(&app, Method::GET, "/operator/dependencies", None).await;
    assert_eq!(StatusCode::UNAUTHORIZED, refused.status);
}
