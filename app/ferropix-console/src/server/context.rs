// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What a page is drawn from, and the gate that reads it.
//!
//! Every page and every route that acts asks for a live session, which
//! `ferrohealth-session`'s middleware left on the request as its
//! [`Held`], and a page calls the FerroPIX server once before it is drawn.
//! No specification governs the console's HTTP surface: our own design.

use axum::Extension;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use ferrohealth_session::record::{Credential, Held, Session};
use ferrohealth_session::return_path::ReturnPath;
use ferrohealth_ui::view::session::sign_in_address;
use http::Method;

use super::{ConsoleState, HEALTH};
use crate::app::{ABOUT, SIGN_IN};
use crate::locale::negotiated;
use crate::model::PageContext;
use crate::paths::{FAVICON, MARK_DARK, MARK_LIGHT, is_sign_in};
use crate::pix::AccessToken;

/// Whether a request for `path` reaches a file or a route that needs no
/// session: FerroUI's assets, the marks, the health routes and the sign-in
/// routes.
#[must_use]
pub fn is_public(path: &str) -> bool {
    path.strip_prefix(ferrohealth_ui::serve::PREFIX)
        .is_some_and(|rest| rest.starts_with('/'))
        || [
            HEALTH,
            ferrohealth_server::health::LIVENESS,
            ferrohealth_server::health::READINESS,
            FAVICON,
            MARK_LIGHT,
            MARK_DARK,
        ]
        .contains(&path)
        || ferrohealth_session::paths::is_own(path)
}

/// The steward's access token a live session holds.
#[must_use]
pub fn token(held: &Held) -> Option<AccessToken> {
    match held {
        Held::SignedIn(session) => bearer(session),
        Held::TokenExpired | Held::Ended | Held::Absent => None,
    }
}

/// The access token `session` sends to the server.
fn bearer(session: &Session) -> Option<AccessToken> {
    match session.credential() {
        Credential::Bearer(token) => Some(AccessToken::new(token.to_secret_string())),
        // NOTE: no specification governs this: our own design; the console signs in through
        // its OpenID Provider alone, so a session carries a bearer token or no credential.
        Credential::Basic { .. } => None,
    }
}

/// Where a request that holds no live session is sent; `None` for a live
/// one.
///
/// That is the sign-in, the sign-in that says the session ended, or the one
/// that says its access token could not be renewed and leads back to
/// `here`.
#[must_use]
pub fn away(held: &Held, here: &str) -> Option<String> {
    match held {
        Held::SignedIn(session) if bearer(session).is_some() => None,
        Held::SignedIn(_) | Held::Absent => Some(String::from(SIGN_IN)),
        Held::Ended => Some(String::from(crate::app::EXPIRED)),
        Held::TokenExpired => Some(sign_in_address(crate::app::UNRENEWED, here)),
    }
}

/// What a request to the page at `path` holding `held` is drawn from.
///
/// The path carries its query. That is the steward's name, the call to the server's liveness route, on About
/// the server's `CapabilityStatement`, and the page's document, with the
/// request's preferences and locale. A request with no live session gets
/// the sign-in as its redirect ([`away`]).
pub async fn page_for(
    state: &ConsoleState,
    held: &Held,
    headers: &http::HeaderMap,
    path: &str,
) -> PageContext {
    let route = path.split(['?', '#']).next().unwrap_or_default().to_owned();
    let signed_out = surrounded(
        PageContext {
            path: path.to_owned(),
            ..PageContext::signed_out(state.preference_prefix())
        },
        state,
        headers,
    );
    if is_sign_in(&route) {
        let back = path.split_once('?').and_then(|(_, query)| {
            url::form_urlencoded::parse(query.as_bytes())
                .find(|(name, _)| name == ferrohealth_ui::view::session::RETURN)
                .and_then(|(_, back)| ReturnPath::parse(&back, SIGN_IN))
                .filter(|back| {
                    !is_sign_in(back.as_str().split(['?', '#']).next().unwrap_or_default())
                })
                .map(|back| back.as_str().to_owned())
        });
        return PageContext { back, ..signed_out };
    }
    let (Held::SignedIn(session), Some(token)) = (held, token(held)) else {
        return PageContext {
            redirect: away(held, path),
            ..signed_out
        };
    };
    let admin = session.scopes().contains(&state.settings().pix.admin_scope);
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
    PageContext {
        operator: session.operator().map(str::to_owned),
        probe: Some(probe),
        capability,
        document,
        admin,
        session: Some(session.left()),
        ..signed_out
    }
}

/// `context` with the request's preferences and locale and the console's
/// own settings a page names.
#[must_use]
pub fn surrounded(
    context: PageContext,
    state: &ConsoleState,
    headers: &http::HeaderMap,
) -> PageContext {
    let prefix = state.preference_prefix();
    let preferences = crate::preferences::kept_cookies(headers, &prefix);
    let configured = state.settings().locale.clone();
    let chosen = crate::locale::chosen(
        &ferrohealth_ui::theme::Preferences::read(&preferences, &prefix),
        headers,
        configured.clone(),
    );
    let provider = state
        .relying_party()
        .and_then(|party| {
            party
                .provider()
                .authorization_endpoint()
                .host_str()
                .map(str::to_owned)
        })
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
pub(super) async fn gate(
    Extension(state): Extension<ConsoleState>,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();
    if is_public(&path) {
        return next.run(request).await;
    }
    let held = request
        .extensions()
        .get::<Held>()
        .cloned()
        .unwrap_or(Held::Absent);
    let read = request
        .uri()
        .path_and_query()
        .map_or_else(|| path.clone(), |read| read.as_str().to_owned());
    let drawn = request.method() == Method::GET || request.method() == Method::HEAD;
    let page = if drawn {
        page_for(&state, &held, request.headers(), &read).await
    } else {
        refused(&state, &held, &path, &read)
    };
    if let Some(to) = &page.redirect {
        // NOTE: no specification governs this: our own design; the search panel asks with
        // `fetch`, which a redirect does not move, so it reads a `401` and loads the sign-in.
        if path == crate::pages::search::SUGGEST {
            return (
                http::StatusCode::UNAUTHORIZED,
                [(http::header::CACHE_CONTROL, "no-store")],
            )
                .into_response();
        }
        return Redirect::to(to).into_response();
    }
    if drawn {
        request.extensions_mut().insert(page);
    }
    next.run(request).await
}

/// Whether a request to `path` that draws no page (a post) holds a live
/// session: its context carries the sign-in as its redirect when it does
/// not, and nothing else is read.
fn refused(state: &ConsoleState, held: &Held, path: &str, read: &str) -> PageContext {
    let mut page = PageContext::signed_out(state.preference_prefix());
    if !is_sign_in(path) {
        page.redirect = away(held, read);
    }
    page
}

#[cfg(test)]
mod tests {
    use super::{is_public, is_sign_in};

    #[test]
    fn only_the_library_assets_the_files_and_the_sign_in_routes_need_no_session() {
        assert!(is_public("/ferrohealth-ui/0.6.2/scripts/fui.js"));
        assert!(is_public("/ferrohealth-ui/0.6.2/themes/pix.css"));
        assert!(is_public("/favicon.svg"));
        assert!(!is_public("/ferrohealth-uix"));
        assert!(is_public("/auth/callback"));
        assert!(!is_public("/"));
        assert!(!is_public("/patients/0192a1b2-0000-7000-8000-000000000000"));
        assert!(is_sign_in("/sign-in"));
        assert!(is_sign_in("/sign-in/expired"));
        assert!(!is_sign_in("/sign-inside"));
    }
}
