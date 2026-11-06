// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A steward signs in to the console through the compose quickstart's
//! development issuer (#497), behind `FERROPIX_E2E`.
//!
//! The issuer is the pinned `mock-oauth2-server` started with the
//! `JSON_CONFIG` of `compose.yaml`, the server runs `quickstart.toml` over
//! PostgreSQL 18, and the console is configured as the compose `console`
//! profile configures it. The case signs in at the provider's login form,
//! as a browser would, and holds the server's answers to the console's
//! operator API calls: a login token whose shape the quickstart's
//! `persons` issuer no longer admits is answered `401` or `403` there. No
//! specification governs the case: our own design.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use axum::extract::Request;
use axum::middleware::{self, Next};
use ferropix_server::fhir::Backend;
use ferropix_server::store::{Job, Store};
use ferropix_testkit::containers::{Postgres, e2e_enabled, postgres, stub_provider_configured};
use http::{StatusCode, header};
use tokio::net::TcpListener;

use super::{app_role, fresh_key};
use crate::auth::application_with;
use crate::support::Secrets;

/// The issuer `quickstart.toml` and the compose console name.
const QUICKSTART_ISSUER: &str = "http://localhost:8180/ferropix";

/// The console's client id at the issuer, as compose sets it.
const CONSOLE_CLIENT: &str = "ferropix-console";

/// The user name the steward types into the provider's login form.
const STEWARD: &str = "synthetic-steward";

/// The `JSON_CONFIG` of the issuer service of `compose.yaml`, as the
/// container receives it: the folded lines joined by one space, and every
/// `$$` compose reads as a literal `$`.
fn compose_issuer_config() -> String {
    let compose =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../compose.yaml"))
            .expect("compose.yaml is read");
    let mut lines = compose.lines();
    let key = lines
        .by_ref()
        .find(|line| line.trim_start() == "JSON_CONFIG: >-")
        .expect("the issuer service sets JSON_CONFIG as a folded scalar");
    let indent = key.len() - key.trim_start().len();
    let folded: Vec<&str> = lines
        .take_while(|line| line.len() - line.trim_start().len() > indent && !line.trim().is_empty())
        .map(str::trim)
        .collect();
    assert!(!folded.is_empty(), "JSON_CONFIG holds a value");
    folded.join(" ").replace("$$", "$")
}

/// The server's answers to the operator API, by path, in the order sent.
type Seen = Arc<Mutex<Vec<(String, StatusCode)>>>;

/// Returns the value of the cookie `name` that `response` sets, as a
/// `Cookie` header pair.
fn cookie(response: &reqwest::Response, name: &str) -> String {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .find(|pair| pair.starts_with(&format!("{name}=")) && *pair != format!("{name}="))
        .unwrap_or_else(|| panic!("the response sets {name}"))
        .to_owned()
}

/// Returns the `Location` of the redirect `response`.
fn location(response: &reqwest::Response) -> String {
    assert!(
        response.status().is_redirection(),
        "a redirect, got {}",
        response.status()
    );
    response
        .headers()
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a Location")
        .to_owned()
}

/// Starts PostgreSQL and the server of `quickstart.toml` trusting `issuer`,
/// served on loopback, and returns its base URL, its operator API answers
/// and what keeps it running.
async fn quickstart_server(issuer: &str) -> (String, Seen, Postgres, tempfile::TempDir) {
    let quickstart =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/quickstart.toml"))
            .expect("quickstart.toml is read");
    assert!(
        quickstart.contains(QUICKSTART_ISSUER),
        "the quickstart's issuer"
    );
    let secrets = Secrets::new();
    let settings = ferropix_server::config::Config::from_sources(
        Some(&quickstart.replace(QUICKSTART_ISSUER, issuer)),
        &secrets.environment(),
    )
    .expect("the quickstart reads")
    .resolve()
    .expect("the quickstart resolves");
    assert!(settings.operator.enabled, "the console's API is mounted");

    let postgres = postgres().await.expect("PostgreSQL starts");
    let url = app_role(&postgres).await;
    let store = Store::start(
        &super::settings(&url, &fresh_key(), true),
        &settings.domains,
        Job::Migrate,
    )
    .await
    .expect("the store starts");
    let backend = Backend {
        store,
        matching: settings.matching.clone(),
        deprecated: settings.pdqm.deprecated,
        configured: ferropix_server::operator::Configured::default(),
        development: false,
    };
    let (app, _smart, spool) = Box::pin(application_with(settings, &backend)).await;
    let seen = Seen::default();
    let recorder = Arc::clone(&seen);
    let app = app.layer(middleware::from_fn(move |request: Request, next: Next| {
        let recorder = Arc::clone(&recorder);
        async move {
            let path = request.uri().path().to_owned();
            let response = next.run(request).await;
            if path.starts_with("/operator/") {
                recorder
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((path, response.status()));
            }
            response
        }
    }));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port for the server");
    let server = format!("http://{}", listener.local_addr().expect("an address"));
    tokio::spawn(async move { axum::serve(listener, app).await });
    (server, seen, postgres, spool)
}

/// Starts the console as the compose `console` profile configures it, at
/// `issuer` and over the server at `server`, and returns its base URL.
async fn compose_console(issuer: &str, server: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port for the console");
    let address = listener.local_addr().expect("an address");
    let console = format!("http://{address}");
    let text = format!(
        "[server]\nlisten = \"{address}\"\n\n\
         [pix]\nbase_url = \"{server}/\"\n\n\
         [session]\nsecure_cookie = false\n\n\
         [oidc]\nissuer = \"{issuer}\"\nauthorization_endpoint = \"{issuer}/authorize\"\n\
         token_endpoint = \"{issuer}/token\"\njwks_uri = \"{issuer}/jwks\"\n\
         end_session_endpoint = \"{issuer}/endsession\"\nclient_id = \"{CONSOLE_CLIENT}\"\n\
         client_secret = \"synthetic\"\nredirect_uri = \"{console}/auth/callback\"\n\
         post_logout_redirect_uri = \"{console}/\"\n"
    );
    let state = ferropix_console::server::ConsoleState::new(
        ferropix_console::config::Config::from_sources(Some(&text), &BTreeMap::new())
            .expect("the console configuration reads")
            .resolve()
            .expect("the console configuration resolves"),
    )
    .await
    .expect("the console's state builds");
    let lifecycle = state.lifecycle().clone();
    tokio::spawn(ferropix_console::server::serve(
        listener,
        ferropix_console::server::router(state),
        lifecycle,
        std::future::pending(),
    ));
    console
}

/// Signs the steward in at `console` through the login form of `issuer`,
/// as a browser does, and returns the session cookie.
async fn sign_in(browser: &reqwest::Client, console: &str, issuer: &str) -> String {
    let begun = browser
        .get(format!("{console}/login"))
        .send()
        .await
        .expect("the console answers /login");
    let sign_in = cookie(&begun, "ferropix_console_sign_in");
    let authorize = location(&begun);
    assert!(
        authorize.starts_with(&format!("{issuer}/authorize?")),
        "{authorize}"
    );
    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("username", STEWARD)
        .append_pair("claims", "")
        .finish();
    let logged_in = browser
        .post(&authorize)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .expect("the provider answers its login form");
    let callback = location(&logged_in);
    assert!(
        callback.starts_with(&format!("{console}/auth/callback?")),
        "{callback}"
    );
    let returned = browser
        .get(&callback)
        .header(header::COOKIE, &sign_in)
        .send()
        .await
        .expect("the console answers its callback");
    assert_eq!(
        StatusCode::SEE_OTHER,
        returned.status(),
        "the console holds the ID Token to its client and begins a session"
    );
    assert_eq!("/", location(&returned));
    cookie(&returned, "ferropix_console_session")
}

#[tokio::test]
async fn a_steward_signs_in_to_the_console_through_the_quickstart_issuer() {
    if !e2e_enabled() {
        return;
    }
    let provider = stub_provider_configured(&compose_issuer_config())
        .await
        .expect("the quickstart's issuer starts");
    let issuer = provider.issuer();
    let (server, seen, _postgres, _spool) = quickstart_server(&issuer).await;
    let console = compose_console(&issuer, &server).await;
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("an HTTP client");
    let session = sign_in(&browser, &console, &issuer).await;

    let dashboard = browser
        .get(format!("{console}/"))
        .header(header::COOKIE, &session)
        .send()
        .await
        .expect("the console draws the dashboard");
    assert_eq!(StatusCode::OK, dashboard.status());
    let seen = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert!(
        seen.iter().any(|(path, _)| path == "/operator/overview"),
        "the dashboard reads the overview: {seen:?}"
    );
    for (path, status) in &seen {
        assert_eq!(
            StatusCode::OK,
            *status,
            "{path}: the server admits the steward's login token as the quickstart's issuer \
             profiles it"
        );
    }
}
