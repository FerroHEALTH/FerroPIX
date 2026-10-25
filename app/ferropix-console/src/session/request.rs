// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The session of a request: whether it carries a live signed-in session,
//! the steward's access token, and the steward's name.

use http::HeaderMap;

use crate::pix::AccessToken;
use crate::server::ConsoleState;
use crate::session::{COOKIE, SessionError};

/// What a request's session cookie names.
#[derive(Debug)]
pub enum Held {
    /// A live signed-in session, with the steward's token and name.
    SignedIn {
        /// The steward's access token.
        token: AccessToken,
        /// The steward's name as the provider gave it.
        operator: Option<String>,
        /// The scopes the provider granted the steward's access token.
        scopes: Vec<String>,
    },
    /// A session cookie whose session has ended.
    Ended,
    /// No session cookie.
    Absent,
}

/// What the request with `headers` holds in `state`'s session store.
///
/// # Errors
/// Returns [`SessionError::Poisoned`] when the store is unusable.
pub fn held(state: &ConsoleState, headers: &HeaderMap) -> Result<Held, SessionError> {
    let Some(id) = state.sessions().cookie_in(headers, COOKIE) else {
        return Ok(Held::Absent);
    };
    match state.sessions().access_token(&id)? {
        Some(token) => Ok(Held::SignedIn {
            token: AccessToken::new(token),
            operator: state.sessions().operator(&id)?,
            scopes: state.sessions().scopes(&id)?,
        }),
        None => Ok(Held::Ended),
    }
}
