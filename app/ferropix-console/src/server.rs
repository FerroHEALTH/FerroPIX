// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's HTTP service, under one set of browser security headers.
//!
//! It serves the health route, the stylesheet, the theme and the brand
//! marks, the sign-in and preference routes, and the Leptos pages and the
//! site bundle.
//!
//! Every response carries the browser security headers of
//! `ferrohealth-server`'s `console`: a Content-Security-Policy whose script
//! source is a nonce minted for that response alone, `nosniff`, `DENY`
//! framing, no referrer and a same-origin opener, and every document is
//! `no-store`. Only the site bundle is compressed. A request that is not a
//! safe method is taken only from the console's own pages (`same_origin`
//! of `ferrohealth-server`), a session's access token is refreshed before the
//! request it arrives with is served, and a page asks for a live session
//! (`gate`) and calls the FerroPIX server once before it is drawn. No
//! specification governs the console's HTTP surface: our own design.

use std::sync::Arc;

use axum::Extension;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use ferrohealth_health::lifecycle::Lifecycle;
use ferrohealth_health::registry::Registry;
use ferrohealth_server::console::{self, ConsoleHeaders, CspNonce};
use ferrohealth_server::health::Health;
use ferrohealth_server::refusal::RefusalBody;
use ferrohealth_server::same_origin::{self, SameOrigin};
use ferrohealth_server::section::ServerSection;
use ferrohealth_server::serve::Drained;
use ferrohealth_smart::metadata::{Http, ServerMetadata};
use ferrohealth_smart::oidc::{OidcError, Provider, RelyingParty};
use ferrohealth_ui::style::{Brand, STYLESHEET as LIBRARY_STYLESHEET};
use http::header::{CACHE_CONTROL, CONTENT_TYPE};
use http::{HeaderValue, Method, StatusCode};
use leptos::config::LeptosOptions;
use leptos::nonce::Nonce;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{And, Predicate as _, SizeAbove};

use crate::app::{ABOUT, SEARCH, SIGN_IN};
use crate::config::settings::{OidcSettings, Settings};
use crate::locale::{Msg, catalog, negotiated, tr_in};
use crate::model::PageContext;
use crate::paths::{FAVICON, MARK_DARK, MARK_LIGHT, STYLESHEET, THEME, is_sign_in};
use crate::pix::{ClientError, Pix};
use crate::session::request::{Held, held};
use crate::session::{COOKIE, Sessions};

/// The path of the console's health summary route.
pub const HEALTH: &str = ferrohealth_server::health::SUMMARY;

/// The path the site bundle is served under.
pub const BUNDLE: &str = "/pkg";

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
    relying_party: Option<RelyingParty>,
    health: Health,
}

/// Why the console's state could not be built.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// A client of the FerroPIX server or of the OpenID Provider could not be
    /// built.
    #[error(transparent)]
    Client(#[from] ClientError),
    /// The configured OpenID Provider endpoints are not ones a relying party
    /// can use.
    #[error("the OpenID Provider's endpoints cannot be used")]
    Provider {
        /// What the relying party refused.
        #[source]
        source: OidcError,
    },
}

impl ConsoleState {
    /// The state for `settings`, with an empty session store.
    ///
    /// # Errors
    /// Returns [`StateError::Client`] when a client of the FerroPIX server, or
    /// of the OpenID Provider, cannot be built, and [`StateError::Provider`]
    /// for provider endpoints a relying party refuses.
    pub fn new(settings: Settings) -> Result<Self, StateError> {
        let pix = Pix::new(&settings.pix)?;
        let relying_party = settings
            .oidc
            .as_ref()
            .map(|oidc| relying_party(oidc, &settings))
            .transpose()?;
        // The settings are resolved and both clients built, so the console
        // serves from here.
        let lifecycle = Lifecycle::default();
        lifecycle.booted();
        Ok(Self {
            sessions: Sessions::new(settings.session),
            settings: Arc::new(settings),
            pix,
            relying_party,
            health: Health::new(Registry::default(), lifecycle),
        })
    }

    /// The indicators and the phase the health routes read; the phase is
    /// serving from construction and draining once a stop is requested.
    #[must_use]
    pub fn health(&self) -> &Health {
        &self.health
    }

    /// The phase of the console.
    #[must_use]
    pub fn lifecycle(&self) -> &Lifecycle {
        self.health.lifecycle()
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

    /// The relying party of the OpenID Provider, which the sign-in exchanges
    /// its code with and a session refreshes its access token with; `None`
    /// when no provider is configured.
    #[must_use]
    pub fn relying_party(&self) -> Option<&RelyingParty> {
        self.relying_party.as_ref()
    }
}

/// The relying party of the provider `oidc` names, calling it within the
/// timeout of a call to the FerroPIX server.
fn relying_party(oidc: &OidcSettings, settings: &Settings) -> Result<RelyingParty, StateError> {
    // NOTE: RFC 6749 §3.2: the token endpoint answers the request itself, so
    // the provider's client follows no redirect.
    let client = reqwest::Client::builder()
        .timeout(settings.pix.timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(ClientError::from)?;
    let metadata = ServerMetadata {
        issuer: oidc.issuer.clone(),
        authorization_endpoint: Some(oidc.authorization_endpoint.to_string()),
        token_endpoint: Some(oidc.token_endpoint.to_string()),
        jwks_uri: Some(oidc.jwks_uri.to_string()),
        end_session_endpoint: oidc.end_session_endpoint.as_ref().map(ToString::to_string),
        ..ServerMetadata::default()
    };
    let provider =
        Provider::from_metadata(&metadata).map_err(|source| StateError::Provider { source })?;
    let mut relying_party = RelyingParty::new(
        Http::from_client(client),
        provider,
        oidc.client_id.clone(),
        oidc.redirect_uri.clone(),
    )
    .with_scopes(oidc.scopes.iter().cloned());
    if let Some(secret) = &oidc.client_secret {
        relying_party = relying_party.with_client_secret(secret.clone());
    }
    if oidc.offline_access {
        relying_party = relying_party.with_offline_access();
    }
    if let Some(redirect) = &oidc.post_logout_redirect_uri {
        relying_party = relying_party.with_post_logout_redirect_uri(redirect.clone());
    }
    Ok(relying_party)
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
    // NOTE: leptos_axum 0.8.10 `init_executor`: a server render spawns its resources on the
    // global executor, which the console starts since it registers its pages itself.
    if any_spawner::Executor::init_tokio().is_err() {
        tracing::debug!("the task executor was already started");
    }
    let options = leptos_options(state.settings());
    let form_action = form_action(state.settings());
    let check = same_origin_check(state.settings());
    let context = {
        let state = state.clone();
        move || {
            leptos::context::provide_context(state.clone());
            provide_request_context();
        }
    };
    let health = ferrohealth_server::health::routes(state.health().clone());
    let pages = axum::Router::new()
        .route_service(HEALTH, health.clone())
        .route_service(ferrohealth_server::health::LIVENESS, health.clone())
        .route_service(ferrohealth_server::health::READINESS, health)
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
        .route(
            crate::paths::SESSION,
            get(crate::session::route::read).post(crate::session::route::extend),
        )
        .route(SEARCH, post(crate::posts::post))
        .route(crate::pages::search::SUGGEST, post(crate::posts::suggest))
        .route(crate::app::AUDIT, post(crate::posts::post))
        .route(crate::pages::trail::EXPORT, post(crate::posts::export))
        .route("/patients/{person}/reveal", post(crate::posts::post))
        .route(crate::app::DOMAINS, post(crate::posts::post))
        .route("/domains/{domain}", post(crate::posts::post))
        .route(crate::app::PSEUDONYM_DOMAINS, post(crate::posts::post))
        .route(
            crate::pages::about::METADATA_DOWNLOAD,
            get(capability_download),
        )
        .route(crate::app::SEARCH_DEMOGRAPHICS, post(crate::posts::post))
        .route(
            "/api/{*function}",
            post({
                let context = context.clone();
                move |request: Request| {
                    leptos_axum::handle_server_fns_with_context(context.clone(), request)
                }
            }),
        );
    let pages = PAGES
        .iter()
        .fold(pages, |pages, path| pages.route(path, get(page)))
        .fallback(leptos_axum::file_and_error_handler_with_context(
            context,
            crate::app::document,
        ))
        .layer(axum::middleware::from_fn(gate))
        .layer(axum::middleware::from_fn(
            crate::oidc::refresh::refresh_session,
        ))
        .layer(axum::middleware::from_fn(move |request, next| {
            same_origin::only(check.clone(), request, next)
        }))
        .layer(Extension(state));
    let headers = ConsoleHeaders::new(form_action).with_bundle(BUNDLE);
    pages
        .layer(bundle_compression())
        .layer(axum::middleware::from_fn(move |request, next| {
            console::headers(headers.clone(), request, next)
        }))
        .with_state(options)
}

/// The same-origin check of the console's unsafe requests: the console's
/// origin is that of the OpenID Connect redirect URI, and a refusal is the
/// catalog's words in the console's locale.
fn same_origin_check(settings: &Settings) -> SameOrigin {
    let locale = settings.locale.clone();
    let body: RefusalBody = Arc::new(move |_uri, _request_id, status, _reason| {
        (
            status,
            [(CACHE_CONTROL, "no-store")],
            tr_in(catalog(), &locale, Msg::ForeignRequestRefused),
        )
            .into_response()
    });
    let check = SameOrigin::new().with_body(body);
    match settings.oidc.as_ref() {
        // NOTE: RFC 6454 §4: `oidc.redirect_uri` resolves only as an http or https URL,
        // whose origin is a tuple, so the fallback is never taken and stays closed.
        Some(oidc) => check
            .clone()
            .with_origin(&oidc.redirect_uri)
            .unwrap_or(check),
        None => check,
    }
}

/// The address patterns of the console's pages, which the browser draws
/// after the first by the console's own route table
/// (`crate::app`'s `route`); an address none names is the not-found page.
pub const PAGES: [&str; 20] = [
    "/",
    "/sign-in",
    "/sign-in/{state}",
    "/queue",
    "/queue/{candidate}",
    "/search",
    "/search/demographics",
    "/patients/{person}",
    "/patients/{person}/merge",
    "/patients/{person}/unmerge",
    "/sources",
    "/sources/{source}",
    "/domains",
    "/domains/{domain}",
    "/pseudonym-domains",
    "/configuration",
    "/audit",
    "/about",
    "/preferences",
    "/preferences/{section}",
];

/// Draws the page at the request's address.
async fn page(Extension(state): Extension<ConsoleState>, request: Request) -> Response {
    let (parts, _) = request.into_parts();
    render(state, parts).await
}

/// Whether a request for `path` reaches a file or a route that needs no
/// session: the site bundle, the stylesheets and marks, the health routes
/// and the sign-in routes.
#[must_use]
pub fn is_public(path: &str) -> bool {
    path.starts_with("/pkg/")
        || [
            HEALTH,
            ferrohealth_server::health::LIVENESS,
            ferrohealth_server::health::READINESS,
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

/// What a request to the page at `path` (with its query) is drawn from.
///
/// That is the steward's name, the call to the server's liveness route, on About
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
        let back = path.split_once('?').and_then(|(_, query)| {
            url::form_urlencoded::parse(query.as_bytes())
                .find(|(name, _)| name == ferrohealth_ui::widget::session::RETURN)
                .and_then(|(_, back)| crate::oidc::return_path(&back).map(str::to_owned))
        });
        return Ok(PageContext { back, ..signed_out });
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
                redirect: Some(String::from(crate::app::EXPIRED)),
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
    let session = match state.sessions().cookie_in(headers, COOKIE) {
        Some(id) => state.sessions().left(&id)?,
        None => None,
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
        session,
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
    // NOTE: no specification governs this: our own design; the session route answers for
    // itself, and a read of it must not count as the steward's activity.
    if is_public(&path) || path.starts_with(SERVER_FUNCTIONS) || path == crate::paths::SESSION {
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
        // NOTE: no specification governs this: our own design; the search panel asks with
        // `fetch`, which a redirect does not move, so it reads a `401` and loads the sign-in.
        if path == crate::pages::search::SUGGEST {
            return (StatusCode::UNAUTHORIZED, [(CACHE_CONTROL, "no-store")]).into_response();
        }
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
        Held::Ended => Some(String::from(crate::app::EXPIRED)),
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
/// request parts it provides carry the one `ferrohealth_server::console`
/// minted, which replaces it.
fn provide_request_context() {
    let Some(parts) = leptos::context::use_context::<http::request::Parts>() else {
        return;
    };
    if let Some(nonce) = parts.extensions.get::<CspNonce>() {
        leptos::context::provide_context(Nonce::from_value(nonce.as_str()));
    }
    if let Some(page) = parts.extensions.get::<PageContext>() {
        leptos::context::provide_context(page.clone());
    }
}

/// The prefix of the console's server functions, which check the session
/// themselves and answer as data, never with a redirect.
pub const SERVER_FUNCTIONS: &str = "/api/";

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

/// An answer of `text` as `media`, revalidated so a rebuilt one is fetched
/// again (RFC 9111 §5.2.2.4).
fn file(media: &'static str, text: &'static str) -> Response {
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(media)),
            (CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ],
        text,
    )
        .into_response()
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

/// Serves `router` on `listener` until `shutdown` resolves, then drains as
/// `ferrohealth-server` does: `lifecycle` turns draining, and the requests in
/// flight get the family's default drain delay and shutdown bound.
///
/// # Errors
/// Returns the I/O error of a listener that fails.
pub async fn serve(
    listener: tokio::net::TcpListener,
    router: axum::Router,
    lifecycle: Lifecycle,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<Drained> {
    let section = ServerSection::default();
    ferrohealth_server::serve::serve(
        listener,
        router,
        lifecycle,
        shutdown,
        section.drain_delay(),
        section.shutdown_timeout(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{is_public, is_sign_in};

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
