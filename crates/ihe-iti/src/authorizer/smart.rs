// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The [`Authorizer`] over the OAuth 2.0 client-credentials token of
//! `ferrohealth-smart` (feature `smart`).
//!
//! [`TokenAuthorizer`] takes each request's access token from a
//! [`TokenSource`], which obtains it with the client-credentials grant (IUA
//! ITI-71 §3.71.4.1.2.1) and renews it before it expires, and incorporates
//! it in the `Authorization` header as a bearer token, marked sensitive
//! (ITI-72 §3.72.4.2). After a `401` it drops the refused token and asks the
//! client for one resend, which obtains a new token (§3.72.4.3).

use std::sync::{Mutex, PoisonError};

use ferrohealth_smart::client::cache::TokenSource;
use http::header::AUTHORIZATION;
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use url::Url;

use super::{Authorized, Authorizer, AuthorizerError, Retry};

/// The [`Authorizer`] that sends the tokens of a [`TokenSource`] as bearer
/// tokens (IUA ITI-72 §3.72.4.2).
///
/// `Debug` names no token: the source hides its tokens and the header kept
/// is marked sensitive.
#[derive(Debug)]
pub struct TokenAuthorizer {
    /// The source of the tokens.
    source: TokenSource,
    /// The `Authorization` value most recently sent, which a `401` drops.
    sent: Mutex<Option<HeaderValue>>,
}

impl TokenAuthorizer {
    /// The authorizer that sends the tokens `source` obtains.
    #[must_use]
    pub fn new(source: TokenSource) -> Self {
        Self {
            source,
            sent: Mutex::new(None),
        }
    }
}

impl Authorizer for TokenAuthorizer {
    fn authorize<'a>(&'a self, _method: &'a Method, _url: &'a Url) -> Authorized<'a> {
        Box::pin(async move {
            let token = self.source.token().await.map_err(AuthorizerError::new)?;
            let mut value = token.header().clone();
            value.set_sensitive(true);
            *self.sent.lock().unwrap_or_else(PoisonError::into_inner) = Some(value.clone());
            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, value);
            Ok(headers)
        })
    }

    fn answered(&self, _url: &Url, status: StatusCode, _headers: &HeaderMap) -> Retry {
        if status != StatusCode::UNAUTHORIZED {
            return Retry::Done;
        }
        // NOTE: IUA ITI-72 §3.72.4.3: the refused token is never sent again; a token another request renewed meanwhile stays, as `TokenSource::invalidate` drops only the one it is handed.
        let sent = self
            .sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(sent) = sent {
            self.source.invalidate(&sent);
        }
        Retry::Resend
    }
}
