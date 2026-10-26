// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FHIR face and the operator API as a SMART App Launch 2.x resource
//! server, over a `wiremock` authorization server.
//!
//! Every route of the permission table is driven with no token, with a token
//! that lacks its scope and with one that holds it; the Source routes also
//! with a Source of another domain. Every signing key is generated when the
//! case runs and lives only in memory.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the key set, the tokens and the answers are read as JSON values"
)]

use std::sync::{Arc, Mutex};

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
use axum::Router;
use axum::body::Body;
use ferropix_server::audit::Audit;
use ferropix_server::auth::Smart;
use ferropix_server::auth::guard::{NOT_PERMITTED, NOT_THE_SOURCE, UNAUTHENTICATED};
use ferropix_server::auth::table::{Need, ROUTES, Route};
use ferropix_server::config::Config;
use ferropix_server::config::settings::Settings;
use ferropix_server::fhir::{self, Mounts};
use ferropix_server::health::Registry;
use ferropix_server::http::router;
use http::{Method, Request, StatusCode, header};
use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use tower::ServiceExt as _;
use wiremock::matchers;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::Secrets;

/// The audience every token is addressed to.
const AUDIENCE: &str = "http://localhost:8080/fhir";

/// The domain every Source case writes to, under the OID arc ITU-T X.660
/// reserves for examples.
pub(crate) const DOMAIN: &str = "urn:oid:2.999.1.1";

/// A made-up identifier value no real person carries, which no log line or
/// refusal may show.
const IDENTIFIER: &str = "SYNTHETIC-ID-4711";

/// The FHIR transactions served today; every other one answers `501` until its
/// issue lands, as does an operator route [`crate::operator::SERVED`] does
/// not name.
const SERVED: [&str; 5] = ["ITI-83", "ITI-104", "ITI-78", "ITI-119", "ITI-94"];

/// Returns whether `status` is an answer of the route's handler: neither a
/// refusal of the guard nor the pending handler's `501`.
fn handled(status: StatusCode) -> bool {
    ![
        StatusCode::UNAUTHORIZED,
        StatusCode::FORBIDDEN,
        StatusCode::NOT_IMPLEMENTED,
    ]
    .contains(&status)
}

/// The scopes that admit every route of the table.
pub(crate) const EVERY_SCOPE: &str =
    "system/Patient.cruds system/Subscription.cruds __ferropix.steward __ferropix.admin";

/// One generated signing key and its public JWK.
struct SigningKey {
    encoding: EncodingKey,
    jwk: serde_json::Value,
}

/// Returns a fresh ES256 key published as `k1`.
fn signing_key() -> SigningKey {
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
            .expect("a key pair");
    let encoding = EncodingKey::from_ec_der(pkcs8.as_ref());
    let mut jwk = serde_json::to_value(
        Jwk::from_encoding_key(&encoding, Algorithm::ES256).expect("a public JWK"),
    )
    .expect("the JWK serializes");
    jwk["kid"] = serde_json::json!("k1");
    jwk["use"] = serde_json::json!("sig");
    SigningKey { encoding, jwk }
}

/// The stub authorization server with its key.
pub(crate) struct Issuer {
    server: MockServer,
    key: SigningKey,
}

impl Issuer {
    /// Starts an issuer serving its RFC 8414 metadata and its key set.
    pub(crate) async fn start() -> Self {
        let server = MockServer::start().await;
        let key = signing_key();
        let uri = server.uri();
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/.well-known/oauth-authorization-server"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issuer": uri,
                "jwks_uri": format!("{uri}/jwks"),
                "token_endpoint": format!("{uri}/token"),
                "token_endpoint_auth_methods_supported": ["private_key_jwt"],
                "grant_types_supported": ["client_credentials"],
            })))
            .mount(&server)
            .await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/jwks"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "keys": [key.jwk.clone()] })),
            )
            .mount(&server)
            .await;
        Self { server, key }
    }

    /// Returns a valid token of `client` granting `scope`.
    pub(crate) fn token(&self, client: &str, scope: &str) -> String {
        let now = jiff::Timestamp::now().as_second();
        let claims = serde_json::json!({
            "iss": self.server.uri(),
            "aud": AUDIENCE,
            "sub": "synthetic-subject-17",
            "client_id": client,
            "iat": now,
            "exp": now + 300,
            "jti": "synthetic-jti-1",
            "scope": scope,
        });
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(String::from("k1"));
        header.typ = Some(String::from("at+jwt"));
        jsonwebtoken::encode(&header, &claims, &self.key.encoding).expect("the token signs")
    }
}

/// Returns the settings of a server trusting `issuer`, every switch on.
pub(crate) fn settings(issuer: &Issuer, secrets: &Secrets) -> Settings {
    let text = format!(
        r#"
[server]
listen = "127.0.0.1:0"
public_base_url = "http://localhost:8080"

[auth]
audience = ["{AUDIENCE}"]

[[auth.issuer]]
issuer = "{issuer}"
clients = ["east-lab", "west-lab"]

[[domain]]
system = "{DOMAIN}"
name = "East hospital"
kind = "local"
source_client_id = "east-lab"

[pixm]
remove_patient = true

[pmir]
enabled = true

[operator]
enabled = true
"#,
        issuer = issuer.server.uri()
    );
    Config::from_sources(Some(&text), &secrets.environment())
        .expect("the configuration reads")
        .resolve()
        .expect("the configuration resolves")
}

/// Returns the backend of `settings`: a store whose connections open on
/// first use, at the unresolvable host the secrets name, so a transaction
/// the guard admits reaches its handler and finds no database.
pub(crate) fn backend(settings: &Settings) -> fhir::Backend {
    fhir::Backend {
        store: ferropix_server::store::Store::lazy(&settings.db).expect("the store is described"),
        matching: settings.matching.clone(),
        deprecated: settings.pdqm.deprecated,
        configured: ferropix_server::operator::Configured::default(),
        development: false,
    }
}

/// Returns the application of `settings` with every route mounted, its
/// audit spool in the temporary directory returned beside it.
pub(crate) async fn application(settings: Settings) -> (Router, Arc<Smart>, tempfile::TempDir) {
    let backend = backend(&settings);
    application_with(settings, &backend).await
}

/// Returns the application of `settings` with every route mounted, its
/// transactions over `backend`, its audit spool in the temporary directory
/// returned beside it.
pub(crate) async fn application_with(
    settings: Settings,
    backend: &fhir::Backend,
) -> (Router, Arc<Smart>, tempfile::TempDir) {
    let smart = Arc::new(
        Box::pin(Smart::discover(settings.auth, &settings.domains))
            .await
            .expect("the authorization server is read"),
    );
    let spool = tempfile::tempdir().expect("a temporary directory");
    let mut audit_settings = settings.audit.clone();
    audit_settings.spool_dir = spool.path().join("spool");
    let audit = Audit::open(&audit_settings, &settings.server.public_base_url)
        .expect("the audit spool opens");
    let health = Registry::default();
    let guarded = fhir::routes(
        &settings.server.base_path,
        &settings.server.public_base_url,
        Mounts::all(),
        &smart,
        &health,
        &audit,
        backend,
    );
    (
        router(&settings.server, &health, Some(guarded)),
        smart,
        spool,
    )
}

/// What one request answered.
pub(crate) struct Answer {
    pub(crate) status: StatusCode,
    pub(crate) challenge: Option<String>,
    pub(crate) body: String,
}

impl Answer {
    /// Returns the body as JSON.
    pub(crate) fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("a JSON body")
    }

    /// Returns the code of the first issue.
    fn issue(&self) -> String {
        self.json()["issue"][0]["code"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    /// Returns the diagnostics of the first issue.
    fn diagnostics(&self) -> String {
        self.json()["issue"][0]["diagnostics"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }
}

/// Sends `method` on `uri` with `authorization`, and reads the answer.
pub(crate) async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    authorization: Option<&str>,
) -> Answer {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(authorization) = authorization {
        request = request.header(header::AUTHORIZATION, authorization);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).expect("a legal request"))
        .await
        .expect("the router answers");
    let status = response.status();
    let challenge = response
        .headers()
        .get(header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("the body reads");
    Answer {
        status,
        challenge,
        body: String::from_utf8(bytes.to_vec()).expect("a UTF-8 body"),
    }
}

/// Returns the request URI of `route`, a Source route naming the domain.
fn uri_of(route: &Route) -> String {
    let path = route.path.replace("{id}", "p-1");
    match route.need {
        Need::Fhir { source: true, .. } => {
            format!(
                "{path}?identifier={}%7C{IDENTIFIER}",
                DOMAIN.replace(':', "%3A")
            )
        }
        _ => path,
    }
}

/// Returns every route a credential is needed for.
fn guarded() -> impl Iterator<Item = &'static Route> {
    ROUTES.iter().filter(|route| route.need != Need::Public)
}

pub(crate) fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

#[tokio::test]
async fn every_guarded_route_answers_a_missing_token_401_with_a_security_issue() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    for route in guarded() {
        let answer = send(&app, route.method.clone(), &uri_of(route), None).await;
        let label = format!("{} {}", route.method, route.path);
        assert_eq!(StatusCode::UNAUTHORIZED, answer.status, "{label}");
        assert_eq!(Some("Bearer"), answer.challenge.as_deref(), "{label}");
        assert_eq!("security", answer.issue(), "{label}");
        assert_eq!(UNAUTHENTICATED, answer.diagnostics(), "{label}");
    }
}

#[tokio::test]
async fn every_guarded_route_answers_a_token_without_its_scope_403_with_a_security_issue() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let token = bearer(&issuer.token("east-lab", "openid system/Practitioner.cruds"));
    for route in guarded() {
        let answer = send(&app, route.method.clone(), &uri_of(route), Some(&token)).await;
        let label = format!("{} {}", route.method, route.path);
        assert_eq!(StatusCode::FORBIDDEN, answer.status, "{label}");
        assert_eq!(
            Some("Bearer error=\"insufficient_scope\""),
            answer.challenge.as_deref(),
            "{label}: RFC 6750 §3.1"
        );
        assert_eq!("security", answer.issue(), "{label}");
        assert_eq!(NOT_PERMITTED, answer.diagnostics(), "{label}");
    }
}

#[tokio::test]
async fn every_guarded_route_admits_the_domains_source_holding_every_scope() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let token = bearer(&issuer.token("east-lab", EVERY_SCOPE));
    for route in guarded() {
        let answer = send(&app, route.method.clone(), &uri_of(route), Some(&token)).await;
        let label = format!("{} {}", route.method, route.path);
        if route.interaction == "operator-dependencies" {
            assert_eq!(StatusCode::OK, answer.status, "{label}: served");
            continue;
        }
        if SERVED.contains(&route.interaction)
            || crate::operator::SERVED.contains(&route.interaction)
        {
            assert!(
                handled(answer.status),
                "{label}: the guard admits it and the Manager or the Supplier answers: {}",
                answer.status
            );
            continue;
        }
        assert_eq!(
            StatusCode::NOT_IMPLEMENTED,
            answer.status,
            "{label}: the guard admits it and the pending handler answers"
        );
        assert_eq!("not-supported", answer.issue(), "{label}");
    }
}

#[tokio::test]
async fn a_source_feeding_another_domain_is_answered_403_with_a_forbidden_issue() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let other = bearer(&issuer.token("west-lab", "system/Patient.cud"));
    let unlisted = bearer(&issuer.token("north-lab", "system/Patient.cud"));
    let sources: Vec<&Route> = ROUTES
        .iter()
        .filter(|route| matches!(route.need, Need::Fhir { source: true, .. }))
        .collect();
    assert_eq!(2, sources.len(), "the ITI-104 update and remove");
    for route in sources {
        for token in [&other, &unlisted] {
            let answer = send(&app, route.method.clone(), &uri_of(route), Some(token)).await;
            let label = format!("{} {}", route.method, route.path);
            assert_eq!(StatusCode::FORBIDDEN, answer.status, "{label}");
            assert_eq!("forbidden", answer.issue(), "{label}");
            assert_eq!(NOT_THE_SOURCE, answer.diagnostics(), "{label}");
            assert_eq!(
                None, answer.challenge,
                "{label}: the scope is not the fault"
            );
        }
        let own = bearer(&issuer.token("east-lab", "system/Patient.cud"));
        let answer = send(&app, route.method.clone(), &uri_of(route), Some(&own)).await;
        assert!(
            handled(answer.status),
            "the Source is admitted and the Manager answers: {}",
            answer.status
        );
    }
}

#[tokio::test]
async fn a_patient_scope_is_refused_and_the_iua_scope_of_a_transaction_admits_it() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let pix = "/fhir/Patient/$ihe-pix";
    let patient = bearer(&issuer.token("east-lab", "patient/Patient.rs"));
    let answer = send(&app, Method::POST, pix, Some(&patient)).await;
    assert_eq!(
        StatusCode::FORBIDDEN,
        answer.status,
        "a patient compartment"
    );
    assert_eq!("security", answer.issue());
    let iua = bearer(&issuer.token("east-lab", "ITI-83"));
    let answer = send(&app, Method::POST, pix, Some(&iua)).await;
    assert_eq!(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        answer.status,
        "PIXm 3.1.0 §2:3.83.5.2: admitted, and the Manager reads a request without a body"
    );
    let answer = send(&app, Method::POST, "/fhir/Patient/$match", Some(&iua)).await;
    assert_eq!(
        StatusCode::FORBIDDEN,
        answer.status,
        "ITI-83 is not ITI-119"
    );
}

#[tokio::test]
async fn an_unknown_path_under_the_guarded_surface_needs_a_token_and_health_does_not() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let answer = send(&app, Method::GET, "/fhir/Practitioner", None).await;
    assert_eq!(StatusCode::UNAUTHORIZED, answer.status);
    let token = bearer(&issuer.token("east-lab", EVERY_SCOPE));
    let answer = send(&app, Method::GET, "/fhir/Practitioner", Some(&token)).await;
    assert_eq!(StatusCode::NOT_FOUND, answer.status);
    let answer = send(&app, Method::GET, "/operator/unknown", None).await;
    assert_eq!(StatusCode::UNAUTHORIZED, answer.status);
    let answer = send(&app, Method::GET, "/health", None).await;
    assert_eq!(StatusCode::OK, answer.status);
}

#[tokio::test]
async fn the_discovery_document_and_the_capability_statement_name_the_token_endpoint_and_scopes() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, smart, _spool) = application(settings(&issuer, &secrets)).await;
    let token_endpoint = format!("{}/token", issuer.server.uri());
    let discovery = send(
        &app,
        Method::GET,
        "/fhir/.well-known/smart-configuration",
        None,
    )
    .await;
    assert_eq!(StatusCode::OK, discovery.status);
    let document = discovery.json();
    assert_eq!(token_endpoint, document["token_endpoint"]);
    let listed: Vec<&str> = document["scopes_supported"]
        .as_array()
        .expect("scopes_supported is listed")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    for scope in smart.scopes_supported() {
        assert!(listed.contains(&scope.as_str()), "{scope}");
    }
    for scope in [
        "system/Patient.rs",
        "system/Patient.cud",
        "__ferropix.steward",
        "ITI-104",
    ] {
        assert!(listed.contains(&scope), "{scope}");
    }
    let metadata = send(&app, Method::GET, "/fhir/metadata", None).await;
    assert_eq!(StatusCode::OK, metadata.status);
    let statement = metadata.json();
    assert_eq!("CapabilityStatement", statement["resourceType"]);
    let security = &statement["rest"][0]["security"];
    assert_eq!("SMART-on-FHIR", security["service"][0]["coding"][0]["code"]);
    assert_eq!(
        "http://fhir-registry.smarthealthit.org/StructureDefinition/oauth-uris",
        security["extension"][0]["url"]
    );
    assert_eq!("token", security["extension"][0]["extension"][0]["url"]);
    assert_eq!(
        token_endpoint,
        security["extension"][0]["extension"][0]["valueUri"]
    );
    let description = security["description"].as_str().expect("a description");
    for scope in smart.scopes_supported() {
        assert!(description.contains(&scope), "{scope}");
    }
}

/// A `tracing` writer into a shared buffer.
#[derive(Clone, Default)]
pub(crate) struct Captured(pub(crate) Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("the buffer").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn a_refusal_shows_no_token_subject_or_identifier_in_its_body_or_the_log() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let (app, _, _spool) = application(settings(&issuer, &secrets)).await;
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let lacking = issuer.token("west-lab", "openid");
    let source = issuer.token("west-lab", "system/Patient.cud");
    let mut forged = issuer.token("east-lab", EVERY_SCOPE);
    forged.push('x');
    let query = format!(
        "/fhir/Patient/$ihe-pix?sourceIdentifier={}%7C{IDENTIFIER}",
        DOMAIN.replace(':', "%3A")
    );
    let feed = format!(
        "/fhir/Patient?identifier={}%7C{IDENTIFIER}",
        DOMAIN.replace(':', "%3A")
    );
    let mut bodies = Vec::new();
    for (method, uri, token) in [
        (Method::GET, query.as_str(), None),
        (Method::GET, query.as_str(), Some(&lacking)),
        (Method::GET, query.as_str(), Some(&forged)),
        (Method::PUT, feed.as_str(), Some(&source)),
    ] {
        let authorization = token.map(|token| bearer(token));
        let answer = send(&app, method, uri, authorization.as_deref()).await;
        assert!(answer.status.is_client_error(), "{uri}: {}", answer.status);
        bodies.push(answer.body);
    }
    let log = String::from_utf8(captured.0.lock().expect("the buffer").clone()).expect("UTF-8");
    assert!(!log.is_empty(), "the gate wrote its audit lines");
    for (place, text) in [("the log", log.as_str()), ("a refusal", &bodies.join("\n"))] {
        assert!(!text.contains(IDENTIFIER), "{place} shows the identifier");
        for token in [&lacking, &source, &forged] {
            assert!(!text.contains(token.as_str()), "{place} shows a token");
        }
    }
    let refusals = bodies.join("\n");
    assert!(
        !refusals.contains("synthetic-subject-17"),
        "a refusal shows the token's subject"
    );
    assert!(!log.contains("sourceIdentifier="), "the log shows a query");
}

#[tokio::test]
async fn a_development_surface_of_basic_users_names_the_basic_service_and_no_discovery() {
    let secrets = Secrets::new();
    let text = format!(
        r#"
profile = "development"

[server]
listen = "127.0.0.1:0"
public_base_url = "http://localhost:8080"

[[auth.basic.users]]
user = "dev-source"
password_hash = "{phc}"
scopes = ["system/Patient.cud"]

[[domain]]
system = "{DOMAIN}"
name = "Development hospital"
kind = "local"
source_client_id = "dev-source"

[dev]
"#,
        phc = crate::support::PHC
    );
    let settings = Config::from_sources(Some(&text), &secrets.environment())
        .expect("the configuration reads")
        .resolve()
        .expect("the development configuration resolves");
    let (app, smart, _spool) = application(settings).await;
    assert_eq!(None, smart.token_endpoint());
    let discovery = send(
        &app,
        Method::GET,
        "/fhir/.well-known/smart-configuration",
        None,
    )
    .await;
    assert_eq!(
        StatusCode::NOT_FOUND,
        discovery.status,
        "no authorization server"
    );
    let metadata = send(&app, Method::GET, "/fhir/metadata", None).await;
    let security = &metadata.json()["rest"][0]["security"];
    assert_eq!("Basic", security["service"][0]["coding"][0]["code"]);
    let answer = send(&app, Method::POST, "/fhir/Patient/$ihe-pix", None).await;
    assert_eq!(StatusCode::UNAUTHORIZED, answer.status);
    assert_eq!(
        Some("Basic realm=\"\", charset=\"UTF-8\""),
        answer.challenge.as_deref(),
        "RFC 7617 §2"
    );
    let wrong = send(
        &app,
        Method::POST,
        "/fhir/Patient/$ihe-pix",
        Some("Basic ZGV2LXNvdXJjZTpub3QtdGhlLXBhc3N3b3Jk"),
    )
    .await;
    assert_eq!(StatusCode::UNAUTHORIZED, wrong.status);
    assert_eq!("security", wrong.issue());
}
