// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's HTTP service, under one set of browser security headers.
//!
//! It serves the health route, the stylesheet, the theme and the brand
//! marks, the sign-in and preference routes, and the Leptos pages and the
//! site bundle.
//!
//! Every response carries a Content-Security-Policy whose script source is a
//! nonce minted for that response alone, `nosniff`, `DENY` framing and no
//! referrer, and every document is `no-store`. Only the site bundle is
//! compressed. A request that is not a safe method is taken only from the
//! console's own pages, a session's access token is refreshed before the
//! request it arrives with is served, and a page asks for a live session
//! (`gate`) and calls the FerroPIX server once before it is drawn. No
//! specification governs the console's HTTP surface: our own design.

use std::sync::Arc;

use axum::Extension;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use ferrohealth_ui::style::{Brand, STYLESHEET as LIBRARY_STYLESHEET};
use http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS,
    X_FRAME_OPTIONS,
};
use http::{HeaderValue, Method, StatusCode};
use leptos::config::LeptosOptions;
use leptos::nonce::Nonce;
use leptos_axum::LeptosRoutes as _;
use serde::Serialize;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{And, Predicate as _, SizeAbove};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::app::{ABOUT, SEARCH, SIGN_IN};
use crate::config::settings::{OidcSettings, Settings};
use crate::locale::{Msg, catalog, negotiated, tr_in};
use crate::model::PageContext;
use crate::paths::{FAVICON, MARK_DARK, MARK_LIGHT, STYLESHEET, THEME, is_sign_in};
use crate::pix::{ClientError, Pix};
use crate::session::Sessions;
use crate::session::request::{Held, held};

/// The path of the console's own liveness route.
pub const HEALTH: &str = "/health";

/// The name cargo-leptos gives the site bundle's files under `pkg/`.
pub const OUTPUT_NAME: &str = "ferropix-console";

/// The FerroPIX favicon, the brand's own file.
const FAVICON_SVG: &str = include_str!("../../../assets/brand/favicon.svg");

/// The FerroPIX mark in the light theme, the brand's own file.
const MARK_LIGHT_SVG: &str = include_str!("../../../assets/brand/ferropix-icon.svg");

/// The FerroPIX mark in the dark theme, the brand's own file.
const MARK_DARK_SVG: &str = include_str!("../../../assets/brand/ferropix-icon-dark.svg");

/// What every request handler shares.
#[derive(Debug, Clone)]
pub struct ConsoleState {
    settings: Arc<Settings>,
    sessions: Sessions,
    pix: Pix,
    provider: reqwest::Client,
}

impl ConsoleState {
    /// The state for `settings`, with an empty session store.
    ///
    /// # Errors
    /// Returns the [`ClientError`] of a client of the FerroPIX server, or of
    /// the OpenID Provider, that cannot be built.
    pub fn new(settings: Settings) -> Result<Self, ClientError> {
        let pix = Pix::new(&settings.pix)?;
        // NOTE: RFC 6749 §3.2: the token endpoint answers the request itself, so
        // the provider's client follows no redirect.
        let provider = reqwest::Client::builder()
            .timeout(settings.pix.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(ClientError::from)?;
        Ok(Self {
            sessions: Sessions::new(settings.session),
            settings: Arc::new(settings),
            pix,
            provider,
        })
    }

    /// The resolved configuration.
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The session store.
    #[must_use]
    pub fn sessions(&self) -> &Sessions {
        &self.sessions
    }

    /// The client of the FerroPIX server.
    #[must_use]
    pub fn pix(&self) -> &Pix {
        &self.pix
    }

    /// The HTTP client of the OpenID Provider, which the sign-in exchanges
    /// its code with and a session refreshes its access token with.
    #[must_use]
    pub fn provider(&self) -> &reqwest::Client {
        &self.provider
    }
}

/// The Leptos options the console renders with: its site bundle under the
/// configured site root and its listen address.
#[must_use]
pub fn leptos_options(settings: &Settings) -> LeptosOptions {
    LeptosOptions::builder()
        .output_name(OUTPUT_NAME)
        .site_root(settings.site_root.to_string_lossy().into_owned())
        .site_pkg_dir("pkg")
        .site_addr(settings.listen)
        .build()
}

/// Builds the console's whole HTTP service over `state`.
pub fn router(state: ConsoleState) -> axum::Router {
    let options = leptos_options(state.settings());
    let form_action = form_action(state.settings());
    let routes = leptos_axum::generate_route_list(crate::app::App);
    let context = {
        let state = state.clone();
        move || {
            leptos::context::provide_context(state.clone());
            provide_request_context();
        }
    };
    let pages = axum::Router::new()
        .route(HEALTH, get(health))
        .route(STYLESHEET, get(stylesheet))
        .route(THEME, get(theme))
        .route(FAVICON, get(favicon))
        .route(MARK_LIGHT, get(mark_light))
        .route(MARK_DARK, get(mark_dark))
        .route(crate::paths::LOGIN, get(crate::oidc::login))
        .route(crate::oidc::CALLBACK, get(crate::oidc::callback))
        .route(
            crate::oidc::logout::LOGOUT,
            post(crate::oidc::logout::logout),
        )
        .route(
            crate::preferences::PREFERENCE_ROUTE,
            post(crate::preferences::choose),
        )
        .route(
            crate::preferences::RESET_ROUTE,
            post(crate::preferences::reset),
        )
        .route(
            crate::preferences::SIDEBAR_ROUTE,
            post(crate::preferences::sidebar),
        )
        .route(SEARCH, post(crate::posts::post))
        .route(crate::app::AUDIT, post(crate::posts::post))
        .route(crate::pages::trail::EXPORT, post(crate::posts::export))
        .route("/patients/{person}/reveal", post(crate::posts::post))
        .route(crate::app::DOMAINS, post(crate::posts::post))
        .route("/domains/{domain}", post(crate::posts::post))
        .route(
            crate::pages::about::METADATA_DOWNLOAD,
            get(capability_download),
        )
        .route(crate::app::SEARCH_DEMOGRAPHICS, post(crate::posts::post))
        .leptos_routes_with_context(&options, routes, context.clone(), {
            let options = options.clone();
            move || crate::app::document(options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler_with_context(
            context,
            crate::app::document,
        ))
        .layer(axum::middleware::from_fn(gate))
        .layer(axum::middleware::from_fn(
            crate::oidc::refresh::refresh_session,
        ))
        .layer(axum::middleware::from_fn(same_origin_only))
        .layer(Extension(state));
    with_security_headers(pages.layer(bundle_compression()), form_action).with_state(options)
}

/// Whether a request for `path` reaches a file or a route that needs no
/// session: the site bundle, the stylesheets and marks, the health route and
/// the sign-in routes.
#[must_use]
pub fn is_public(path: &str) -> bool {
    path.starts_with("/pkg/")
        || [
            HEALTH,
            STYLESHEET,
            THEME,
            FAVICON,
            MARK_LIGHT,
            MARK_DARK,
            crate::paths::LOGIN,
            crate::oidc::CALLBACK,
            crate::oidc::logout::LOGOUT,
        ]
        .contains(&path)
}

/// What a request to the page at `path` (with its query) is drawn from:
/// the steward's name, the call to the server's liveness route, on About
/// the server's `CapabilityStatement`, and the page's document, with the
/// request's preferences and locale. A request with no live session gets
/// the sign-in as its redirect, one whose session has ended the sign-in
/// that says so.
///
/// # Errors
///
/// Returns the session store's error when it cannot be read.
pub async fn page_for(
    state: &ConsoleState,
    headers: &http::HeaderMap,
    path: &str,
) -> Result<PageContext, crate::session::SessionError> {
    let route = path.split(['?', '#']).next().unwrap_or_default().to_owned();
    let prefix = state.sessions().preference_prefix();
    let signed_out = surrounded(
        PageContext {
            path: path.to_owned(),
            ..PageContext::signed_out(prefix.clone())
        },
        state,
        headers,
    );
    if is_sign_in(&route) {
        return Ok(signed_out);
    }
    let (token, operator, admin) = match held(state, headers)? {
        Held::SignedIn {
            token,
            operator,
            scopes,
        } => {
            let admin = scopes.contains(&state.settings().pix.admin_scope);
            (token, operator, admin)
        }
        Held::Ended => {
            return Ok(PageContext {
                redirect: Some(format!("{SIGN_IN}/expired")),
                ..signed_out
            });
        }
        Held::Absent => {
            return Ok(PageContext {
                redirect: Some(String::from(SIGN_IN)),
                ..signed_out
            });
        }
    };
    let probe = state.pix().probe(&token).await;
    let capability = if route == ABOUT {
        Some(state.pix().capability_statement(&token).await)
    } else {
        None
    };
    let document = if probe.is_up() {
        crate::document::load(state.pix(), &token, &route).await
    } else {
        None
    };
    Ok(PageContext {
        operator,
        probe: Some(probe),
        capability,
        document,
        admin,
        ..signed_out
    })
}

/// `context` with the request's preferences and locale and the console's
/// own settings a page names.
#[must_use]
pub fn surrounded(
    context: PageContext,
    state: &ConsoleState,
    headers: &http::HeaderMap,
) -> PageContext {
    let prefix = state.sessions().preference_prefix();
    let preferences = crate::preferences::kept_cookies(headers, &prefix);
    let configured = state.settings().locale.clone();
    let chosen = crate::locale::chosen(
        &ferrohealth_ui::theme::Preferences::read(&preferences, &prefix),
        headers,
        configured.clone(),
    );
    let provider = state
        .settings()
        .oidc
        .as_ref()
        .and_then(|oidc| oidc.authorization_endpoint.host_str().map(str::to_owned))
        .unwrap_or_default();
    PageContext {
        preferences,
        locale: chosen.tag(),
        browser: negotiated(headers, configured).tag(),
        base: state.pix().base().to_string(),
        provider,
        sign_in: state.settings().oidc.is_some(),
        today: today(),
        prefix,
        ..context
    }
}

/// Today's year, month and day in UTC, `None` for a clock outside the years
/// a `u16` holds.
fn today() -> Option<(u16, u8, u8)> {
    let date = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date();
    Some((
        u16::try_from(date.year()).ok()?,
        u8::try_from(date.month()).ok()?,
        u8::try_from(date.day()).ok()?,
    ))
}

/// Holds every page and every route that acts to a live signed-in session,
/// and calls the FerroPIX server once before a page is drawn
/// ([`page_for`]).
async fn gate(
    Extension(state): Extension<ConsoleState>,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();
    if is_public(&path) || path.starts_with(SERVER_FUNCTIONS) {
        return next.run(request).await;
    }
    let read = request
        .uri()
        .path_and_query()
        .map_or_else(|| path.clone(), |read| read.as_str().to_owned());
    let drawn = request.method() == Method::GET || request.method() == Method::HEAD;
    let page = if drawn {
        page_for(&state, request.headers(), &read).await
    } else {
        refused(&state, request.headers(), &path)
    };
    let page = match page {
        Ok(page) => page,
        Err(error) => {
            tracing::error!(%error, "the session store could not be read");
            let locale = negotiated(request.headers(), state.settings().locale.clone());
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(CACHE_CONTROL, "no-store")],
                tr_in(catalog(), &locale, Msg::SessionUnreadable),
            )
                .into_response();
        }
    };
    if let Some(to) = &page.redirect {
        return Redirect::to(to).into_response();
    }
    if drawn {
        request.extensions_mut().insert(page);
    }
    next.run(request).await
}

/// Whether a request that draws no page (a post) holds a live session: its
/// context carries the sign-in as its redirect when it does not, and
/// nothing else is read.
fn refused(
    state: &ConsoleState,
    headers: &http::HeaderMap,
    path: &str,
) -> Result<PageContext, crate::session::SessionError> {
    let mut page = PageContext::signed_out(state.sessions().preference_prefix());
    if is_sign_in(path) {
        return Ok(page);
    }
    page.redirect = match held(state, headers)? {
        Held::SignedIn { .. } => None,
        Held::Ended => Some(format!("{SIGN_IN}/expired")),
        Held::Absent => Some(String::from(SIGN_IN)),
    };
    Ok(page)
}

/// Draws the page at the address of `parts`, whose extensions carry the
/// [`PageContext`] a form post's handler read, as a `GET` of it renders.
pub async fn render(state: ConsoleState, parts: http::request::Parts) -> Response {
    let options = leptos_options(state.settings());
    let context = move || {
        leptos::context::provide_context(state.clone());
        provide_request_context();
    };
    let handler = leptos_axum::render_app_to_stream_with_context(context, {
        let options = options.clone();
        move || crate::app::document(options.clone())
    });
    handler(Request::from_parts(parts, axum::body::Body::empty())).await
}

/// Serves the server's `CapabilityStatement` as a file, About's Download:
/// read with the steward's token as About reads it, and `502` when the
/// server does not send one.
async fn capability_download(
    Extension(state): Extension<ConsoleState>,
    headers: http::HeaderMap,
) -> Response {
    let Ok(Held::SignedIn { token, .. }) = held(&state, &headers) else {
        return Redirect::to(SIGN_IN).into_response();
    };
    match state.pix().capability_statement(&token).await {
        crate::model::Capability::Declared(text) => (
            [
                (
                    CONTENT_TYPE,
                    HeaderValue::from_static("application/fhir+json"),
                ),
                (
                    http::header::CONTENT_DISPOSITION,
                    HeaderValue::from_static("attachment; filename=\"capability-statement.json\""),
                ),
            ],
            text,
        )
            .into_response(),
        crate::model::Capability::Missing(_) => StatusCode::BAD_GATEWAY.into_response(),
    }
}

/// Provides the request's nonce and its [`PageContext`] to the render.
///
/// `leptos_axum` provides a nonce of its own before this runs, and the
/// request parts it provides carry the one [`content_security_policy`]
/// minted, which replaces it.
fn provide_request_context() {
    let Some(parts) = leptos::context::use_context::<http::request::Parts>() else {
        return;
    };
    if let Some(nonce) = parts.extensions.get::<Nonce>() {
        leptos::context::provide_context(nonce.clone());
    }
    if let Some(page) = parts.extensions.get::<PageContext>() {
        leptos::context::provide_context(page.clone());
    }
}

/// The prefix of the console's server functions, which check the session
/// themselves and answer as data, never with a redirect.
pub const SERVER_FUNCTIONS: &str = "/api/";

/// The fetch metadata header that names where a request comes from.
const SEC_FETCH_SITE: &str = "sec-fetch-site";

/// Refuses every request that is not a safe method, a sign-out and a
/// preference post among them, unless it comes from the console's own pages
/// ([`same_origin`]), before anything reads the session.
///
/// The session cookie is `SameSite=Lax` as well, so a cross-site `POST`
/// carries no session in the first place; this check holds without it.
async fn same_origin_only(
    Extension(state): Extension<ConsoleState>,
    request: Request,
    next: Next,
) -> Response {
    if request.method().is_safe() {
        return next.run(request).await;
    }
    let origin = state.settings().oidc.as_ref().map(OidcSettings::origin);
    if same_origin(request.headers(), origin.as_deref()) {
        return next.run(request).await;
    }
    tracing::warn!(
        method = %request.method(),
        "a request that did not come from the console's own pages was refused"
    );
    let locale = negotiated(request.headers(), state.settings().locale.clone());
    (
        StatusCode::FORBIDDEN,
        [(CACHE_CONTROL, "no-store")],
        tr_in(catalog(), &locale, Msg::ForeignRequestRefused),
    )
        .into_response()
}

/// Whether `headers` show a request sent from a page of the console at
/// `origin`, its own `scheme://host:port`.
///
/// `Sec-Fetch-Site` decides when the browser sends it, and only
/// `same-origin` passes. Without it `Origin` must be `origin`, and without
/// that the origin of `Referer` must be. A request that shows none of them,
/// or a console with no known origin, is refused, so an unsafe request is
/// never taken on trust (the W3C Fetch Metadata Request Headers; RFC 6454
/// §7).
#[must_use]
pub fn same_origin(headers: &http::HeaderMap, origin: Option<&str>) -> bool {
    if let Some(site) = headers.get(SEC_FETCH_SITE) {
        return site.as_bytes() == b"same-origin";
    }
    let Some(origin) = origin else {
        return false;
    };
    if let Some(sent) = headers.get(http::header::ORIGIN) {
        return sent.as_bytes() == origin.as_bytes();
    }
    // NOTE: RFC 9110 §10.1.3: a `Referer` that is no URL names no origin, which
    // refuses the request as a missing one does.
    headers
        .get(http::header::REFERER)
        .and_then(|referer| referer.to_str().ok())
        .and_then(|referer| url::Url::parse(referer).ok())
        .is_some_and(|referer| referer.origin().ascii_serialization() == origin)
}

/// The sources of the policy's `form-action`: the console itself, and the
/// origin of the provider's end-session endpoint when one is configured.
///
/// A browser holds every redirect a form submission follows to
/// `form-action` (Content Security Policy Level 3, `form-action`), and the
/// sign-out form is answered with a redirect to that endpoint (OpenID
/// Connect RP-Initiated Logout 1.0 §2), so without its origin the browser
/// blocks the sign-out on its way to the provider.
#[must_use]
pub fn form_action(settings: &Settings) -> String {
    let end_session = settings
        .oidc
        .as_ref()
        .and_then(|oidc| oidc.end_session_endpoint.as_ref())
        .map(url::Url::origin)
        .filter(url::Origin::is_tuple);
    match end_session {
        Some(origin) => format!("'self' {}", origin.ascii_serialization()),
        None => String::from("'self'"),
    }
}

/// The body of the liveness route.
#[derive(Debug, Serialize)]
struct Health {
    /// Always `up`: the route answers while the process serves.
    status: &'static str,
}

/// An answer of `text` as `media`.
fn file(media: &'static str, text: &'static str) -> Response {
    ([(CONTENT_TYPE, HeaderValue::from_static(media))], text).into_response()
}

/// Serves the library's stylesheet.
async fn stylesheet() -> Response {
    file("text/css; charset=utf-8", LIBRARY_STYLESHEET)
}

/// Serves the library's FerroPIX theme.
async fn theme() -> Response {
    file("text/css; charset=utf-8", Brand::Pix.theme())
}

/// Serves the favicon.
async fn favicon() -> Response {
    file("image/svg+xml", FAVICON_SVG)
}

/// Serves the brand mark in the light theme.
async fn mark_light() -> Response {
    file("image/svg+xml", MARK_LIGHT_SVG)
}

/// Serves the brand mark in the dark theme.
async fn mark_dark() -> Response {
    file("image/svg+xml", MARK_DARK_SVG)
}

/// Serves `GET /health`: `200` while the process serves.
async fn health() -> Response {
    (StatusCode::OK, axum::Json(Health { status: "up" })).into_response()
}

/// The media types of the site bundle: the WebAssembly, its JavaScript glue
/// and the stylesheet.
const BUNDLE_TYPES: [&str; 4] = [
    "application/wasm",
    "text/javascript",
    "application/javascript",
    "text/css",
];

/// A test of a response, the shape the compression layer's predicate takes.
type ResponsePredicate = fn(StatusCode, http::Version, &http::HeaderMap, &http::Extensions) -> bool;

/// Whether a response carries a file of the site bundle, by its media type.
fn is_bundle(
    _status: StatusCode,
    _version: http::Version,
    headers: &http::HeaderMap,
    _extensions: &http::Extensions,
) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|media| BUNDLE_TYPES.iter().any(|bundle| media.starts_with(bundle)))
}

/// Compresses the site bundle with brotli or gzip, as the request's
/// `Accept-Encoding` chooses, and marks it `Vary: Accept-Encoding`.
///
/// Only the bundle is compressed. A document can carry what the steward
/// entered beside data an attacker would want, so it is never compressed,
/// which keeps a compression side channel (BREACH) shut. The Leptos book
/// (`deployment/binary_size`) has a site serve its WebAssembly compressed.
fn bundle_compression() -> CompressionLayer<And<SizeAbove, ResponsePredicate>> {
    let bundle: ResponsePredicate = is_bundle;
    CompressionLayer::new()
        .br(true)
        .gzip(true)
        .compress_when(SizeAbove::new(256).and(bundle))
}

/// Wraps `router` in the browser security headers every response carries,
/// with `form_action` as the policy's `form-action` sources.
fn with_security_headers(
    router: axum::Router<LeptosOptions>,
    form_action: String,
) -> axum::Router<LeptosOptions> {
    let form_action: Arc<str> = Arc::from(form_action);
    router
        .layer(axum::middleware::from_fn(
            move |request: Request, next: Next| {
                content_security_policy(Arc::clone(&form_action), request, next)
            },
        ))
        .layer(axum::middleware::from_fn(cache_control))
        .layer(SetResponseHeaderLayer::overriding(
            X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
}

/// Mints this response's script nonce, hands it to the renderer through the
/// request, and answers with the policy that names it and `form_action`.
async fn content_security_policy(
    form_action: Arc<str>,
    mut request: Request,
    next: Next,
) -> Response {
    let nonce = Nonce::new();
    let policy = HeaderValue::from_str(&policy(&nonce, &form_action));
    request.extensions_mut().insert(nonce);
    let mut response = next.run(request).await;
    match policy {
        Ok(policy) => {
            response
                .headers_mut()
                .insert(CONTENT_SECURITY_POLICY, policy);
            response
        }
        Err(_invalid) => {
            tracing::error!("the Content-Security-Policy could not be written as a header");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// The Content-Security-Policy of one response, naming its script nonce and
/// the `form-action` sources [`form_action`] returns.
///
/// The WebAssembly bundle needs `'wasm-unsafe-eval'` to be compiled, which
/// admits no JavaScript `eval` (Content Security Policy Level 3). Scripts run
/// by nonce alone, styles come from the console alone, and nothing admits
/// `'unsafe-inline'`.
#[must_use]
pub fn policy(nonce: &Nonce, form_action: &str) -> String {
    format!(
        "default-src 'self'; \
         script-src 'self' 'wasm-unsafe-eval' 'nonce-{nonce}'; \
         style-src 'self'; \
         img-src 'self' data:; \
         connect-src 'self'; \
         object-src 'none'; \
         base-uri 'self'; \
         form-action {form_action}; \
         frame-ancestors 'none'"
    )
}

/// Marks every document `no-store`, and the site bundle, the stylesheets
/// and the marks `no-cache`, so a rebuilt one is fetched again (RFC 9111
/// §5.2.2).
async fn cache_control(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let asset = path.starts_with("/pkg/")
        || [STYLESHEET, THEME, FAVICON, MARK_LIGHT, MARK_DARK].contains(&path);
    let mut response = next.run(request).await;
    let value = if asset && response.status().is_success() {
        "no-cache"
    } else {
        "no-store"
    };
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(value));
    response
}

/// Serves `router` on `listener` until `shutdown` resolves.
///
/// # Errors
/// Returns the I/O error of a listener that fails.
pub async fn serve(
    listener: tokio::net::TcpListener,
    router: axum::Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

#[cfg(test)]
mod tests {
    use leptos::nonce::Nonce;

    use super::{is_public, is_sign_in, policy};

    #[test]
    fn the_policy_allows_scripts_only_by_nonce_and_never_eval() {
        let nonce = Nonce::new();
        let policy = policy(&nonce, "'self'");
        let script = policy
            .split(';')
            .map(str::trim)
            .find(|directive| directive.starts_with("script-src"))
            .expect("a script-src directive");
        assert_eq!(
            format!("script-src 'self' 'wasm-unsafe-eval' 'nonce-{nonce}'"),
            script
        );
        assert!(policy.contains("style-src 'self';"), "{policy}");
        assert!(!policy.contains("'unsafe-eval'"), "{policy}");
        assert!(!policy.contains("'unsafe-inline'"), "{policy}");
    }

    #[test]
    fn only_the_bundle_the_files_and_the_sign_in_routes_need_no_session() {
        assert!(is_public("/pkg/ferropix-console.wasm"));
        assert!(is_public("/theme.css"));
        assert!(is_public("/auth/callback"));
        assert!(!is_public("/"));
        assert!(!is_public("/patients/0192a1b2-0000-7000-8000-000000000000"));
        assert!(is_sign_in("/sign-in"));
        assert!(is_sign_in("/sign-in/expired"));
        assert!(!is_sign_in("/sign-inside"));
    }
}
