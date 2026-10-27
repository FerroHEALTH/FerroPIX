// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Operator sign-in at an OpenID Provider: the OAuth 2.0 authorization code
//! grant with PKCE, run by the console's server half through
//! `ferrohealth_smart::oidc::RelyingParty`.
//!
//! `GET /login` mints the `state`, the `nonce` and the PKCE verifier, keeps
//! them in the bounded pool of pending sign-ins, and redirects the browser to
//! the provider's authorization endpoint (RFC 6749 §4.1.1, RFC 7636 §4.3,
//! OpenID Connect Core 1.0 §3.1.2.1). `GET /auth/callback` is where the
//! provider sends the operator back: it takes the pending sign-in once, and
//! the relying party holds the returned `state` to it (RFC 6749 §10.12),
//! trades the code for the operator's tokens and checks the ID Token; only
//! then is a signed-in session begun. [`refresh`] renews its access token
//! before it expires, and [`logout`] ends it. The browser never sees a
//! token. The session store is the console's: no specification governs it,
//! our own design.

use axum::Extension;
use axum::extract::Query;
use axum::response::{IntoResponse, Response};
use ferrohealth_secret::secret::Secret;
use ferrohealth_smart::oidc::{Callback, OidcError, PendingSignIn};
use ferrohealth_ui::i18n::Locale;
use http::header::{CACHE_CONTROL, LOCATION, SET_COOKIE};
use http::{HeaderMap, HeaderValue, StatusCode};

use crate::locale::{Msg, catalog, negotiated, tr_in};
use crate::server::ConsoleState;
use crate::session::{COOKIE, SIGN_IN_COOKIE, SessionError, SignedIn};

pub mod logout;
pub mod refresh;

/// The path of the redirection endpoint the provider sends the operator back
/// to.
pub const CALLBACK: &str = "/auth/callback";

/// Serves `GET /login`: begins a sign-in and redirects to the provider.
pub async fn login(Extension(state): Extension<ConsoleState>, headers: HeaderMap) -> Response {
    let locale = locale_of(&state, &headers);
    let Some(relying_party) = state.relying_party() else {
        return plain(
            StatusCode::SERVICE_UNAVAILABLE,
            &locale,
            Msg::SignInNotConfigured,
        );
    };
    let request = match relying_party.authorization_request() {
        Ok(request) => request,
        Err(error) => {
            tracing::error!(%error, "a sign-in could not be minted");
            return plain(
                StatusCode::INTERNAL_SERVER_ERROR,
                &locale,
                Msg::SignInSessionNotBegun,
            );
        }
    };
    let location = request.url;
    let id = match state.sessions().begin(request.pending) {
        Ok(id) => id,
        Err(error) => return refused(&locale, &error),
    };
    let cookie = state.sessions().sign_in_cookie(&id).to_string();
    let (Ok(location), Ok(cookie)) = (
        HeaderValue::from_str(location.as_str()),
        HeaderValue::from_str(&cookie),
    ) else {
        tracing::error!("the sign-in redirect could not be written as headers");
        return plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            &locale,
            Msg::SignInRedirectUnwritten,
        );
    };
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(LOCATION, location);
    headers.insert(SET_COOKIE, cookie);
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Serves `GET /auth/callback`: holds the returned `state` to the pending
/// sign-in the browser's cookie names, and completes it.
pub async fn callback(
    Extension(state): Extension<ConsoleState>,
    headers: HeaderMap,
    Query(query): Query<Callback>,
) -> Response {
    let locale = locale_of(&state, &headers);
    let Some(id) = state.sessions().cookie_in(&headers, SIGN_IN_COOKIE) else {
        return plain(StatusCode::BAD_REQUEST, &locale, Msg::NoSignInPending);
    };
    let mut response = match state.sessions().take_pending(&id) {
        Ok(Some(pending)) => redirected(&state, &headers, &pending, &query).await,
        Ok(None) => plain(StatusCode::BAD_REQUEST, &locale, Msg::NoSignInPending),
        Err(error) => return refused(&locale, &error),
    };
    // NOTE: no specification governs this: our own design; the sign-in is
    // spent either way, so the browser is told to drop its cookie.
    if let Ok(removal) =
        HeaderValue::from_str(&state.sessions().sign_in_cookie_removal().to_string())
    {
        response.headers_mut().append(SET_COOKIE, removal);
    }
    response
}

/// The answer to a redirect back for the pending sign-in `pending`: the
/// code exchanged, the ID Token checked, and a signed-in session begun.
async fn redirected(
    state: &ConsoleState,
    headers: &HeaderMap,
    pending: &PendingSignIn,
    query: &Callback,
) -> Response {
    let locale = &locale_of(state, headers);
    let Some(relying_party) = state.relying_party() else {
        return plain(
            StatusCode::SERVICE_UNAVAILABLE,
            locale,
            Msg::SignInNotConfigured,
        );
    };
    let signed_in = match relying_party.exchange(pending, query).await {
        Ok(signed_in) => SignedIn {
            operator: signed_in.claims.display_name().map(str::to_owned),
            access_token: signed_in.access_token.to_secret_string(),
            expires_in: signed_in.expires_in,
            refresh_token: signed_in
                .refresh_token
                .as_ref()
                .map(Secret::to_secret_string),
            id_token: Some(signed_in.id_token.to_secret_string()),
            scopes: signed_in.scopes,
        },
        // NOTE: RFC 6749 §10.12: a redirect whose `state` is not the one the
        // request carried is refused before anything else it says is read.
        Err(OidcError::StateMismatch) => {
            return plain(StatusCode::BAD_REQUEST, locale, Msg::SignInStateMismatch);
        }
        Err(OidcError::Authorization { .. }) => {
            return plain(StatusCode::UNAUTHORIZED, locale, Msg::ProviderDidNotSignIn);
        }
        Err(OidcError::NoCode) => {
            return plain(StatusCode::BAD_REQUEST, locale, Msg::RedirectCarriesNoCode);
        }
        Err(error) if error.refuses_the_operator() => {
            tracing::warn!(error = %error, "a sign-in's ID Token was refused");
            return plain(StatusCode::UNAUTHORIZED, locale, Msg::IdTokenDidNotVerify);
        }
        Err(error) => {
            tracing::error!(
                error = %crate::command::chain(&error),
                "a sign-in could not be completed at the OpenID Provider"
            );
            return plain(StatusCode::BAD_GATEWAY, locale, Msg::SignInNotCompleted);
        }
    };
    // NOTE: no specification governs this: our own design; a new sign-in ends
    // the session the browser held, so signing in again never fills the pool.
    if let Some(replaced) = state.sessions().cookie_in(headers, COOKIE)
        && let Err(error) = state.sessions().end(&replaced)
    {
        return refused(locale, &error);
    }
    let id = match state.sessions().establish(signed_in) {
        Ok(id) => id,
        Err(error) => return refused(locale, &error),
    };
    let Ok(cookie) = HeaderValue::from_str(&state.sessions().session_cookie(&id).to_string())
    else {
        tracing::error!("the session cookie could not be written as a header");
        return plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            locale,
            Msg::SessionNotBegun,
        );
    };
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(LOCATION, HeaderValue::from_static("/"));
    headers.insert(SET_COOKIE, cookie);
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The answer to a session store that failed.
fn refused(locale: &Locale, error: &SessionError) -> Response {
    match error {
        SessionError::Full => plain(StatusCode::SERVICE_UNAVAILABLE, locale, Msg::SessionsFull),
        SessionError::Random | SessionError::Poisoned => {
            tracing::error!(%error, "a sign-in session could not be begun");
            plain(
                StatusCode::INTERNAL_SERVER_ERROR,
                locale,
                Msg::SignInSessionNotBegun,
            )
        }
    }
}

/// A plain-text answer that no cache keeps.
fn plain(status: StatusCode, locale: &Locale, msg: Msg) -> Response {
    let text = tr_in(catalog(), locale, msg);
    (status, [(CACHE_CONTROL, "no-store")], text).into_response()
}

/// The locale of a request to a sign-in route with `headers`.
fn locale_of(state: &ConsoleState, headers: &HeaderMap) -> Locale {
    negotiated(headers, state.settings().locale.clone())
}
