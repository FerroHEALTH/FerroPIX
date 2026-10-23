// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What the tests share: settings built from a configuration text, the
//! console's service, one request through it, a sign-in completed at the
//! test OpenID Provider, and the log lines a test captures.

use std::collections::BTreeMap;
use std::error::Error;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use axum::body::Body;
use http::{Request, Response, StatusCode};
use tower::ServiceExt as _;
use url::Url;

use crate::provider::{IdTokenClaims, Provider, TokenAnswer};
use ferropix_console::config::Config;
use ferropix_console::config::settings::Settings;
use ferropix_console::server::{ConsoleState, router};

/// The settings a console with its site bundle under `site_root` and its
/// FerroPIX server at `server` runs on.
pub(crate) fn settings(site_root: &Path, server: &str) -> Settings {
    let text = format!(
        "[server]\nlisten = \"127.0.0.1:0\"\nsite_root = {site_root:?}\n\n[pix]\nbase_url = \"{server}\"\n",
        site_root = site_root.to_string_lossy(),
    );
    Config::from_sources(Some(&text), &BTreeMap::new())
        .expect("the configuration reads")
        .resolve()
        .expect("the configuration resolves")
}

/// The console's client id at the test provider.
pub(crate) const CLIENT_ID: &str = "ferropix-console";

/// The console's redirection endpoint, on loopback.
pub(crate) const REDIRECT_URI: &str = "http://127.0.0.1:3000/auth/callback";

/// The console's own origin, the redirection endpoint's.
pub(crate) const ORIGIN: &str = "http://127.0.0.1:3000";

/// The synthetic access token every test operator signs in with.
pub(crate) const OPERATOR_TOKEN: &str = "synthetic-operator-access-token";

/// The synthetic refresh token a test operator's sign-in leaves.
pub(crate) const OPERATOR_REFRESH: &str = "synthetic-operator-refresh-token";

/// The name the test provider gives every operator.
pub(crate) const OPERATOR_NAME: &str = "Synthetic Operator";

/// The configuration of a console that signs in at `provider` and calls the
/// FerroPIX server at `server`, with the TOML lines `oidc` and `session` added to
/// those sections.
pub(crate) fn configuration(
    provider: &Provider,
    server: &str,
    oidc: &str,
    session: &str,
) -> String {
    format!(
        "[pix]\nbase_url = \"{server}\"\n\n\
         [session]\nsecure_cookie = false\n{session}\n\n\
         [oidc]\nissuer = \"{issuer}\"\n\
         authorization_endpoint = \"{authorization}\"\n\
         token_endpoint = \"{token}\"\n\
         jwks_uri = \"{jwks}\"\n\
         client_id = \"{CLIENT_ID}\"\n\
         redirect_uri = \"{REDIRECT_URI}\"\n{oidc}\n",
        issuer = provider.issuer(),
        authorization = provider.authorization_endpoint(),
        token = provider.token_endpoint(),
        jwks = provider.jwks_uri(),
    )
}

/// Resolves the configuration `text` writes, with no environment.
pub(crate) fn resolve(text: &str) -> Result<Settings, Box<dyn Error>> {
    Ok(Config::from_sources(Some(text), &BTreeMap::new())?.resolve()?)
}

/// The state and the service for the configuration `text` writes.
pub(crate) fn console(text: &str) -> Result<(ConsoleState, axum::Router), Box<dyn Error>> {
    let state = ConsoleState::new(resolve(text)?)?;
    let service = router(state.clone());
    Ok((state, service))
}

/// Sends `request` through `service` and reads the whole answer.
pub(crate) async fn send(
    service: &axum::Router,
    request: Request<Body>,
) -> Result<(Response<()>, String), Box<dyn Error>> {
    let response = service.clone().oneshot(request).await?;
    let (parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await?;
    Ok((
        Response::from_parts(parts, ()),
        String::from_utf8(bytes.to_vec())?,
    ))
}

/// A `GET` of `path` with no body.
pub(crate) fn get(path: &str) -> Result<Request<Body>, Box<dyn Error>> {
    Ok(Request::get(path).body(Body::empty())?)
}

/// A `GET` of `path` carrying `cookie`, a `name=value` pair.
pub(crate) fn get_with(path: &str, cookie: &str) -> Result<Request<Body>, Box<dyn Error>> {
    Ok(Request::get(path)
        .header("cookie", cookie)
        .body(Body::empty())?)
}

/// The value of the header `name` on `response`, or an empty string.
pub(crate) fn header<'a>(response: &'a Response<()>, name: &str) -> &'a str {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

/// Every `Set-Cookie` of `response`.
pub(crate) fn set_cookies(response: &Response<()>) -> Vec<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(str::to_owned)
        .collect()
}

/// One sign-in begun at the console: its cookie, and the query of the
/// authorization request it redirected to.
pub(crate) struct Begun {
    /// The `name=value` pair of the pending sign-in's cookie.
    pub(crate) cookie: String,
    /// The authorization request's query pairs.
    pub(crate) query: BTreeMap<String, String>,
    /// The authorization request.
    pub(crate) location: Url,
}

impl Begun {
    /// The query pair `name`, or an empty string.
    pub(crate) fn pair(&self, name: &str) -> &str {
        self.query.get(name).map(String::as_str).unwrap_or_default()
    }
}

/// Begins a sign-in at `service` with `GET /login`.
pub(crate) async fn begin(service: &axum::Router) -> Result<Begun, Box<dyn Error>> {
    let (response, _body) = send(service, get("/login")?).await?;
    assert_eq!(
        StatusCode::SEE_OTHER,
        response.status(),
        "the sign-in redirects"
    );
    let location = Url::parse(header(&response, "location"))?;
    let query = location.query_pairs().into_owned().collect();
    let cookie = header(&response, "set-cookie")
        .split(';')
        .next()
        .ok_or("a sign-in cookie")?
        .to_owned();
    Ok(Begun {
        cookie,
        query,
        location,
    })
}

/// Sends the provider's redirect back for `begun` with `code`.
pub(crate) async fn complete(
    service: &axum::Router,
    begun: &Begun,
) -> Result<(Response<()>, String), Box<dyn Error>> {
    let request = Request::get(format!(
        "/auth/callback?code=synthetic-code&state={}",
        begun.pair("state")
    ))
    .header("cookie", &begun.cookie)
    .body(Body::empty())?;
    send(service, request).await
}

/// The `name=value` pair of the session cookie `response` sets.
pub(crate) fn session_cookie(response: &Response<()>) -> Result<String, Box<dyn Error>> {
    set_cookies(response)
        .into_iter()
        .find(|cookie| {
            cookie.starts_with("ferropix_console_session=") && !cookie.contains("Max-Age=0")
        })
        .and_then(|cookie| cookie.split(';').next().map(str::to_owned))
        .ok_or_else(|| "a session cookie".into())
}

/// Signs in at `service` through `provider`, which answers the code with
/// `answer` and an ID Token for the sign-in's `nonce`, and returns the
/// session cookie.
pub(crate) async fn sign_in(
    service: &axum::Router,
    provider: &Provider,
    answer: TokenAnswer,
) -> Result<String, Box<dyn Error>> {
    let begun = begin(service).await?;
    let claims = IdTokenClaims::new(&provider.issuer(), CLIENT_ID, begun.pair("nonce"));
    let answer = TokenAnswer {
        id_token: Some(provider.id_token(&claims)?),
        ..answer
    };
    provider.reset().await?;
    provider.answer_code(&answer).await?;
    let (response, body) = complete(service, &begun).await?;
    assert_eq!(StatusCode::SEE_OTHER, response.status(), "{body}");
    session_cookie(&response)
}

/// Every line the console logs while it is captured.
#[derive(Clone, Default)]
pub(crate) struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    /// Captures every event of this thread, at every level, until the guard
    /// drops.
    pub(crate) fn logging(&self) -> tracing::subscriber::DefaultGuard {
        let writer = self.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::set_default(subscriber)
    }

    /// What was logged.
    pub(crate) fn text(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The configuration of a console that calls the FerroPIX server at
/// `server` and signs in nowhere: its sessions are begun by
/// [`steward_session`] in the store itself.
pub(crate) fn offline(server: &str) -> String {
    format!(
        "[pix]\nbase_url = \"{server}\"\ntimeout_ms = 2000\n\n[session]\nsecure_cookie = false\n"
    )
}

/// Begins a signed-in session for the synthetic steward in `state`'s store,
/// as a completed sign-in would, and returns its `name=value` cookie.
pub(crate) fn steward_session(state: &ConsoleState) -> String {
    let id = state
        .sessions()
        .establish(ferropix_console::session::SignedIn {
            access_token: secrecy::SecretString::from(OPERATOR_TOKEN),
            expires_in: None,
            refresh_token: None,
            id_token: None,
            operator: Some(String::from(OPERATOR_NAME)),
        })
        .expect("a session begins");
    format!("ferropix_console_session={}", id.as_str())
}

/// A `POST` of the form `body` to `path`, from the console's own page,
/// carrying `cookie`.
pub(crate) fn post_form(
    path: &str,
    cookie: &str,
    body: &str,
) -> Result<Request<Body>, Box<dyn Error>> {
    Ok(Request::post(path)
        .header("cookie", cookie)
        .header("sec-fetch-site", "same-origin")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body.to_owned()))?)
}
