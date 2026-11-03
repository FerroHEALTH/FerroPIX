// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The session route the page's session watch asks ([`crate::paths::SESSION`]).
//!
//! It answers on the wire of FerroUI `docs/consoles.md`, "The session's
//! end": `GET` answers what the session has left and never counts as the
//! steward's activity, `POST` counts as it and starts the idle timeout
//! again, both as `{"idle": …, "absolute": …}` in whole seconds, and `401`
//! says the session is gone. No specification governs the session store:
//! our own design.

use axum::Extension;
use axum::response::{IntoResponse as _, Response};
use http::header::CACHE_CONTROL;
use http::{HeaderMap, Method, StatusCode};

use crate::locale::messages::Msg;
use crate::locale::{catalog, negotiated, tr_in};
use crate::server::ConsoleState;
use crate::session::COOKIE;

/// Serves the session route: what the session the request's cookie names
/// has left, `401` with no live session.
///
/// `GET` reads it without touching the idle timeout; `POST` first counts the
/// steward's own activity, which starts the idle timeout again.
pub async fn session(
    method: Method,
    Extension(state): Extension<ConsoleState>,
    headers: HeaderMap,
) -> Response {
    let extend = method == Method::POST;
    let Some(id) = state.sessions().cookie_in(&headers, COOKIE) else {
        return gone();
    };
    match state.sessions().left(&id, extend) {
        Ok(Some(left)) => ([(CACHE_CONTROL, "no-store")], axum::Json(left)).into_response(),
        Ok(None) => gone(),
        Err(error) => {
            tracing::error!(%error, "the session store could not be read");
            let locale = negotiated(&headers, state.settings().locale.clone());
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(CACHE_CONTROL, "no-store")],
                tr_in(catalog(), &locale, Msg::SessionUnreadable),
            )
                .into_response()
        }
    }
}

/// The answer for a session that is gone.
fn gone() -> Response {
    (StatusCode::UNAUTHORIZED, [(CACHE_CONTROL, "no-store")]).into_response()
}
