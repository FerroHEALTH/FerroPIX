// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's HTTP service, under one set of browser security headers.
//!
//! It serves the health route, FerroUI's stylesheet, theme and script
//! modules, the brand marks, the sign-in, preference and post routes, and
//! the pages, each drawn whole on the server.
//!
//! Every response carries the browser security headers of
//! `ferrohealth-server`'s `console` (`nosniff`, `DENY` framing, no referrer
//! and a same-origin opener, every document `no-store`) under the strict
//! policy of FerroUI `docs/consoles.md`: scripts and styles from the
//! console's own origin alone, with no nonce. Only FerroUI's assets are
//! compressed. A request that is not a
//! safe method is taken only from the console's own pages (`same_origin`
//! of `ferrohealth-server`), a session's access token is refreshed before the
//! request it arrives with is served, and a page asks for a live session
//! ([`context`]'s `gate`) and calls the FerroPIX server once before it is drawn. No
//! specification governs the console's HTTP surface: our own design.

use std::sync::Arc;

use axum::Extension;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use ferrohealth_health::lifecycle::Lifecycle;
use ferrohealth_health::registry::Registry;
use ferrohealth_server::console::{self, ConsoleHeaders};
use ferrohealth_server::health::Health;
use ferrohealth_server::refusal::RefusalBody;
use ferrohealth_server::same_origin::{self, SameOrigin};
use ferrohealth_server::section::ServerSection;
use ferrohealth_server::serve::Drained;
use ferrohealth_smart::metadata::{Http, ServerMetadata};
use ferrohealth_smart::oidc::{OidcError, Provider, RelyingParty};
use ferrohealth_ui::serve::{PREFIX, axum_router};
use http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use http::{HeaderValue, Method, StatusCode};
use tower_http::compression::CompressionLayer;

use crate::app::SIGN_IN;
use crate::config::settings::{OidcSettings, Settings};
use crate::locale::messages::Msg;
use crate::locale::{catalog, tr_in};
use crate::model::PageContext;
use crate::paths::{FAVICON, MARK_DARK, MARK_LIGHT};
use crate::pix::{ClientError, Pix};
use crate::session::Sessions;
use crate::session::request::{Held, held};

pub mod context;

/// The path of the console's health summary route.
pub const HEALTH: &str = ferrohealth_server::health::SUMMARY;

/// The FerroPIX favicon, the brand's own file.
const FAVICON_SVG: &str = include_str!("../../../../assets/brand/favicon.svg");

/// The FerroPIX mark in the light theme, the brand's own file.
const MARK_LIGHT_SVG: &str = include_str!("../../../../assets/brand/ferropix-icon.svg");

/// The FerroPIX mark in the dark theme, the brand's own file.
const MARK_DARK_SVG: &str = include_str!("../../../../assets/brand/ferropix-icon-dark.svg");

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

/// Builds the console's whole HTTP service over `state`.
pub fn router(state: ConsoleState) -> axum::Router {
    let form_action = form_action(state.settings());
    let check = same_origin_check(state.settings());
    let health = ferrohealth_server::health::routes(state.health().clone());
    let pages = axum::Router::new()
        .merge(health)
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
        .route(crate::pages::search::SUGGEST, post(crate::posts::suggest))
        .route(crate::pages::trail::EXPORT, post(crate::posts::export))
        .route(
            crate::pages::about::METADATA_DOWNLOAD,
            get(capability_download),
        );
    let pages = PAGES
        .iter()
        .fold(pages, |pages, path| pages.route(path, get(page)));
    let pages = POSTS
        .iter()
        .fold(pages, |pages, path| {
            pages.route(path, post(crate::posts::post))
        })
        .fallback(page)
        .layer(axum::middleware::from_fn(context::gate))
        .layer(axum::middleware::from_fn(
            crate::oidc::refresh::refresh_session,
        ))
        .layer(axum::middleware::from_fn(move |request, next| {
            same_origin::only(check.clone(), request, next)
        }))
        .layer(Extension(state));
    // NOTE: FerroUI `docs/consoles.md`, "How a console builds": the library's assets
    // carry their own one-year immutable cache, which the console's headers keep.
    let assets = axum_router::router().layer(asset_compression());
    let headers = ConsoleHeaders::new(form_action.clone());
    pages
        .nest(PREFIX, assets)
        .layer(axum::middleware::from_fn(move |request, next| {
            console::headers(headers.clone(), request, next)
        }))
        .layer(axum::middleware::from_fn(move |request, next| {
            strict_policy(form_action.clone(), request, next)
        }))
}

/// The routes a page's forms post to, each answered by
/// [`crate::posts::post`] with the page drawn again.
const POSTS: [&str; 11] = [
    "/search",
    "/search/demographics",
    "/audit",
    "/queue",
    "/queue/{candidate}",
    "/patients/{person}/merge",
    "/patients/{person}/unmerge",
    "/patients/{person}/reveal",
    "/domains",
    "/domains/{domain}",
    "/pseudonym-domains",
];

/// The Content-Security-Policy of every response, forms posting to
/// `form_action`.
///
/// Scripts and styles come from the console's own origin alone, with no
/// nonce and no `'wasm-unsafe-eval'` (FerroUI `docs/consoles.md`, "How a
/// console builds", step 6; W3C Content Security Policy Level 3).
#[must_use]
pub fn policy(form_action: &str) -> String {
    format!(
        "default-src 'self'; \
         script-src 'self'; \
         style-src 'self'; \
         img-src 'self' data:; \
         connect-src 'self'; \
         object-src 'none'; \
         base-uri 'self'; \
         form-action {form_action}; \
         frame-ancestors 'none'"
    )
}

/// Writes [`policy`] over the one `console::headers` sets for a hydrated
/// console.
// TODO(#389): take the server-rendered policy from `ferrohealth-server` once
// FerroHEALTH/FerroSYS#202 ships it, and drop this layer.
async fn strict_policy(form_action: String, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    match HeaderValue::from_str(&policy(&form_action)) {
        Ok(value) => {
            response
                .headers_mut()
                .insert(CONTENT_SECURITY_POLICY, value);
        }
        Err(_invalid) => {
            // NOTE: no specification governs this: our own design; a policy that cannot be a
            // header falls back to the strictest one, which still blocks scripts.
            response.headers_mut().insert(
                CONTENT_SECURITY_POLICY,
                HeaderValue::from_static("default-src 'none'"),
            );
            tracing::error!("the Content-Security-Policy could not be written as a header");
        }
    }
    response
}

/// The same-origin check of the console's unsafe requests: the console's
/// origin is that of the OpenID Connect redirect URI, and a refusal is the
/// catalog's words in the console's locale.
fn same_origin_check(settings: &Settings) -> SameOrigin {
    let locale = settings.locale.clone();
    let body: RefusalBody = Arc::new(move |_uri, _ids, status, _reason| {
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

/// The address patterns of the console's pages, each drawn whole on the
/// server (`crate::app`'s `route`); an address none names is the
/// not-found page.
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

/// Draws the page at the request's address, from the [`PageContext`] the
/// `gate` read; a request that reads no page (a post to an address with no
/// form) is `404`.
async fn page(request: Request) -> Response {
    let (parts, _) = request.into_parts();
    if parts.method != Method::GET && parts.method != Method::HEAD {
        return StatusCode::NOT_FOUND.into_response();
    }
    render(&parts)
}

/// Draws the page whose [`PageContext`] the extensions of `parts` carry:
/// the one the `gate` read for a `GET`, or the one a form post's handler
/// read with the server's answer.
#[must_use]
pub fn render(parts: &http::request::Parts) -> Response {
    let Some(page) = parts.extensions.get::<PageContext>().cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (status, html) = crate::app::page(page);
    (status, axum::response::Html(String::from(html))).into_response()
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

/// Compresses FerroUI's assets with brotli or gzip, as the request's
/// `Accept-Encoding` chooses, and marks them `Vary: Accept-Encoding`.
///
/// Only the assets are compressed. A document can carry what the steward
/// entered beside data an attacker would want, so it is never compressed,
/// which keeps a compression side channel (BREACH) shut.
fn asset_compression() -> CompressionLayer {
    CompressionLayer::new().br(true).gzip(true)
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
