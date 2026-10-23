// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Operator sign-out at `POST /logout`.
//!
//! It ends the server-side session, removes the session cookie, and sends
//! the operator to the provider's end-session endpoint when one is
//! configured (OpenID Connect RP-Initiated Logout 1.0 §2).
//!
//! A sign-out is a `POST`, which the server takes only from the console's own
//! pages, as it takes every request that is not a safe method
//! ([`crate::server::same_origin`]).

use axum::Extension;
use axum::response::{IntoResponse, Response};
use http::header::{CACHE_CONTROL, LOCATION, SET_COOKIE};
use http::{HeaderMap, HeaderValue, StatusCode};
use secrecy::{ExposeSecret as _, SecretString};
use url::Url;

use crate::config::settings::OidcSettings;
use crate::locale::Msg;
use crate::server::ConsoleState;
use crate::session::COOKIE;

/// The path of the sign-out route.
pub const LOGOUT: &str = crate::app::SIGN_OUT;

/// The provider's end-session request for `oidc`.
///
/// It is the endpoint with the ID Token hint, the console's client id, and
/// the registered post-logout redirect, appended to whatever query the
/// endpoint already carries (OpenID Connect RP-Initiated Logout 1.0 §2).
///
/// Returns `None` when the provider publishes no end-session endpoint.
#[must_use]
pub fn end_session_request(oidc: &OidcSettings, id_token: Option<&SecretString>) -> Option<Url> {
    let mut url = oidc.end_session_endpoint.clone()?;
    {
        let mut query = url.query_pairs_mut();
        if let Some(id_token) = id_token {
            query.append_pair("id_token_hint", id_token.expose_secret());
        }
        query.append_pair("client_id", &oidc.client_id);
        if let Some(redirect) = &oidc.post_logout_redirect_uri {
            query.append_pair("post_logout_redirect_uri", redirect.as_str());
        }
    }
    Some(url)
}

/// Serves `POST /logout`: ends the session the browser holds, removes its
/// cookie, and redirects to the provider's end-session endpoint or to the
/// landing page.
pub async fn logout(Extension(state): Extension<ConsoleState>, headers: HeaderMap) -> Response {
    let locale = super::locale_of(&state, &headers);
    let Some(oidc) = state.settings().oidc.as_ref() else {
        return super::plain(
            StatusCode::SERVICE_UNAVAILABLE,
            &locale,
            Msg::SignInNotConfigured,
        );
    };
    let sessions = state.sessions();
    let held = sessions.cookie_in(&headers, COOKIE);
    let id_token = match held.map(|id| sessions.end(&id)).transpose() {
        Ok(ended) => ended.flatten(),
        Err(error) => {
            tracing::error!(%error, "a session could not be ended");
            return super::plain(
                StatusCode::INTERNAL_SERVER_ERROR,
                &locale,
                Msg::SessionNotEnded,
            );
        }
    };
    let location = end_session_request(oidc, id_token.as_ref()).map_or_else(
        || format!("{}/signed-out", crate::app::SIGN_IN),
        String::from,
    );
    let removal = sessions.session_cookie_removal().to_string();
    let (Ok(location), Ok(removal)) = (
        HeaderValue::from_str(&location),
        HeaderValue::from_str(&removal),
    ) else {
        tracing::error!("the sign-out redirect could not be written as headers");
        return super::plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            &locale,
            Msg::SignOutRedirectUnwritten,
        );
    };
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(LOCATION, location);
    headers.insert(SET_COOKIE, removal);
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
