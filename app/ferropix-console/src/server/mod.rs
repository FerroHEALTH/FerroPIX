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
//! policy of a server-rendered console (`ConsoleHeaders::server_rendered`,
//! FerroUI `docs/consoles.md`): scripts and styles from the console's own
//! origin alone, with no nonce. Only FerroUI's assets are
//! compressed. A request that is not a
//! safe method is taken only from the console's own pages (`same_origin`
//! of `ferrohealth-server`). The sign-in, callback, sign-out and session
//! routes and the middleware that refreshes a session's access token before
//! the request it arrives with is served are `ferrohealth-session`'s, which
//! draws a stopped sign-in through [`pages::SignInPages`]; a page asks for a
//! live session ([`context`]'s `gate`) and calls the FerroPIX server once
//! before it is drawn. No specification governs the console's HTTP surface:
//! our own design.

use std::sync::Arc;

use axum::Extension;
use axum::extract::Request;
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
use ferrohealth_session::clock::Clock;
use ferrohealth_session::cookies::{CookieNames, HOST_PREFIX};
use ferrohealth_session::record::Held;
use ferrohealth_session::routes::{SignIn, keep_session};
use ferrohealth_session::sessions::Sessions;
use ferrohealth_smart::oidc::RelyingParty;
use ferrohealth_ui::serve::{PREFIX, axum_router};
use http::header::{CACHE_CONTROL, CONTENT_TYPE};
use http::{HeaderValue, Method, StatusCode};
use tower_http::compression::CompressionLayer;

use crate::app::SIGN_IN;
use crate::config::settings::Settings;
use crate::locale::messages::Msg;
use crate::locale::{catalog, tr_in};
use crate::model::PageContext;
use crate::paths::{FAVICON, MARK_DARK, MARK_LIGHT};
use crate::pix::{ClientError, Pix};

pub mod context;
pub mod pages;

/// The base name of every cookie the console sets: `ferropix_console_session`
/// carries the session, `ferropix_console_sign_in` a pending sign-in, and
/// `ferropix_console_theme` the theme.
pub const COOKIES: &str = "ferropix_console";

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
    sign_in: SignIn<pages::SignInPages>,
    pix: Pix,
    health: Health,
}

/// Why the console's state could not be built.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// A client of the FerroPIX server could not be built.
    #[error(transparent)]
    Client(#[from] ClientError),
    /// The relying party at the OpenID Provider could not be built: its
    /// client, its discovery, or endpoints it cannot use.
    #[error("the OpenID Provider cannot be used")]
    Provider {
        /// What the relying party refused.
        #[source]
        source: Box<ferrohealth_session::config::ConfigError>,
    },
}

impl ConsoleState {
    /// The state for `settings`, with an empty session store; the relying
    /// party discovers the provider's endpoints first when the configuration
    /// does not name them.
    ///
    /// # Errors
    /// Returns [`StateError::Client`] when the client of the FerroPIX server
    /// cannot be built, and [`StateError::Provider`] for a relying party
    /// that cannot be built or a provider that cannot be read.
    pub async fn new(settings: Settings) -> Result<Self, StateError> {
        Self::with_clock(settings, Clock::system()).await
    }

    /// The state for `settings`, as [`ConsoleState::new`] builds it, with a
    /// session store that counts every lifetime on `clock`.
    ///
    /// # Errors
    /// Returns the errors of [`ConsoleState::new`].
    pub async fn with_clock(settings: Settings, clock: Clock) -> Result<Self, StateError> {
        let pix = Pix::new(&settings.pix)?;
        let party =
            match &settings.oidc {
                Some(oidc) => Some(oidc.checked.relying_party().await.map_err(|source| {
                    StateError::Provider {
                        source: Box::new(source),
                    }
                })?),
                None => None,
            };
        let sessions = Sessions::with_clock(
            settings.session.clone(),
            CookieNames::for_console(COOKIES),
            clock,
        );
        let sign_in = SignIn::new(
            sessions,
            party,
            pages::SignInPages::new(settings.locale.clone()),
        );
        // The settings are resolved and both clients built, so the console
        // serves from here.
        let lifecycle = Lifecycle::default();
        lifecycle.booted();
        Ok(Self {
            settings: Arc::new(settings),
            sign_in,
            pix,
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

    /// The sign-in: the session store and the relying party.
    #[must_use]
    pub fn sign_in(&self) -> &SignIn<pages::SignInPages> {
        &self.sign_in
    }

    /// The session store.
    #[must_use]
    pub fn sessions(&self) -> &Sessions {
        self.sign_in.sessions()
    }

    /// The client of the FerroPIX server.
    #[must_use]
    pub fn pix(&self) -> &Pix {
        &self.pix
    }

    /// The relying party of the OpenID Provider; `None` when no provider is
    /// configured.
    #[must_use]
    pub fn relying_party(&self) -> Option<&RelyingParty> {
        self.sign_in.party()
    }

    /// The name every preference cookie starts with on this console, under
    /// the `__Host-` prefix when the cookies are `Secure`.
    #[must_use]
    pub fn preference_prefix(&self) -> String {
        let base = format!("{COOKIES}_");
        if self.settings.session.secure_cookie() {
            format!("{HOST_PREFIX}{base}")
        } else {
            base
        }
    }
}

/// Builds the console's whole HTTP service over `state`.
pub fn router(state: ConsoleState) -> axum::Router {
    let form_action = form_action(&state);
    let check = same_origin_check(state.settings());
    let health = ferrohealth_server::health::routes(state.health().clone());
    let pages = axum::Router::new()
        .merge(health)
        .route(FAVICON, get(brand))
        .route(MARK_LIGHT, get(brand))
        .route(MARK_DARK, get(brand))
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
        .layer(axum::middleware::from_fn_with_state(
            state.sign_in().clone(),
            keep_session::<pages::SignInPages>,
        ))
        .merge(ferrohealth_session::routes::router(state.sign_in().clone()))
        .layer(axum::middleware::from_fn(move |request, next| {
            same_origin::only(check.clone(), request, next)
        }))
        .layer(Extension(state));
    // NOTE: FerroUI `docs/consoles.md`, "How a console builds": the library's assets
    // carry their own one-year immutable cache, which the console's headers keep.
    let assets = axum_router::router().layer(asset_compression());
    let headers = ConsoleHeaders::server_rendered(form_action);
    pages
        .nest(PREFIX, assets)
        .layer(axum::middleware::from_fn(move |request, next| {
            console::headers(headers.clone(), request, next)
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
    let check = SameOrigin::default().with_body(body);
    match settings.oidc.as_ref() {
        // NOTE: RFC 6454 §4: `oidc.redirect_uri` resolves only as an http or https URL,
        // whose origin is a tuple, so the fallback is never taken and stays closed.
        Some(oidc) => check
            .clone()
            .with_origin(oidc.redirect_uri())
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
    Extension(held): Extension<Held>,
) -> Response {
    let Some(token) = context::token(&held) else {
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
pub fn form_action(state: &ConsoleState) -> String {
    let end_session = state
        .relying_party()
        .and_then(|party| party.provider().end_session_endpoint())
        .map(url::Url::origin)
        .filter(url::Origin::is_tuple);
    match end_session {
        Some(origin) => format!("'self' {}", origin.ascii_serialization()),
        None => String::from("'self'"),
    }
}

/// The brand marks the console serves, by route.
const BRAND: [(&str, &str); 3] = [
    (FAVICON, FAVICON_SVG),
    (MARK_LIGHT, MARK_LIGHT_SVG),
    (MARK_DARK, MARK_DARK_SVG),
];

/// Serves the brand mark the request's path names as SVG, revalidated so a
/// rebuilt one is fetched again (RFC 9111 §5.2.2.4).
async fn brand(uri: http::Uri) -> Response {
    let Some((_, svg)) = BRAND.iter().find(|(route, _)| uri.path().ends_with(route)) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static("image/svg+xml")),
            (CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ],
        *svg,
    )
        .into_response()
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
        section.drain_delay.get(),
        section.shutdown_timeout.get(),
    )
    .await
}
