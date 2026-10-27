// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What a page is drawn from, and the gate that reads it.
//!
//! Every page and every route that acts asks for a live session, and a page
//! calls the FerroPIX server once before it is drawn. No specification
//! governs the console's HTTP surface: our own design.

use axum::Extension;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use http::header::CACHE_CONTROL;
use http::{Method, StatusCode};

use super::{ConsoleState, HEALTH, SERVER_FUNCTIONS};
use crate::app::{ABOUT, SIGN_IN};
use crate::locale::messages::Msg;
use crate::locale::{catalog, negotiated, tr_in};
use crate::model::PageContext;
use crate::paths::{FAVICON, MARK_DARK, MARK_LIGHT, STYLESHEET, THEME, is_sign_in};
use crate::session::COOKIE;
use crate::session::request::{Held, held};

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
pub(super) async fn gate(
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
