// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The [`Authorizer`] over the OAuth 2.0 client-credentials token of
//! `ferrohealth-smart` (feature `smart`).
//!
//! [`TokenAuthorizer`] takes each request's access token from a
//! [`TokenSource`], which obtains it with the client-credentials grant (IUA
//! ITI-71 §3.71.4.1.2.1) and renews it before it expires, and incorporates
//! it in the `Authorization` header, marked sensitive (ITI-72 §3.72.4.2):
//! as a bearer token, or as a `DPoP` token with a proof of the request when
//! the source's grant binds its tokens to a `DPoP` key (RFC 9449 §7.1).
//! After a `401` that demands a `DPoP` nonce it keeps the token and asks the
//! client for one resend, whose proof carries the nonce (§9); after any
//! other `401` it drops the refused token and asks for one resend, which
//! obtains a new token (ITI-72 §3.72.4.3).

use std::sync::{Mutex, PoisonError};

use ferrohealth_smart::client::binding::IssuedToken;
use ferrohealth_smart::client::cache::TokenSource;
use ferrohealth_smart::client::dpop::{self, DpopError, Role};
use http::header::AUTHORIZATION;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use url::Url;

use super::{Authorized, Authorizer, AuthorizerError, Retry};

/// The [`Authorizer`] that sends the tokens of a [`TokenSource`], as bearer
/// tokens (IUA ITI-72 §3.72.4.2) or as `DPoP` tokens each request proves
/// (RFC 9449 §7.1).
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

/// Returns the `DPoP` header value proving a `method` request to `url`
/// under `token`, or `None` for a token that is not `DPoP`-bound.
fn proof(
    token: &IssuedToken,
    method: &Method,
    url: &Url,
) -> Result<Option<HeaderValue>, DpopError> {
    token
        .proof(method, url)?
        .map(|proof| {
            HeaderValue::try_from(proof).map(|mut value| {
                value.set_sensitive(true);
                value
            })
        })
        .transpose()
        .map_err(|_invalid| DpopError::Header)
}

impl Authorizer for TokenAuthorizer {
    fn authorize<'a>(&'a self, method: &'a Method, url: &'a Url) -> Authorized<'a> {
        Box::pin(async move {
            let token = self.source.token().await.map_err(AuthorizerError::new)?;
            let issued: &IssuedToken = token.as_ref().as_ref();
            let proof = proof(issued, method, url).map_err(AuthorizerError::new)?;
            let mut value = token.header().clone();
            value.set_sensitive(true);
            *self.sent.lock().unwrap_or_else(PoisonError::into_inner) = Some(value.clone());
            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, value);
            if let Some(proof) = proof {
                headers.insert(HeaderName::from_static("dpop"), proof);
            }
            Ok(headers)
        })
    }

    fn answered(&self, url: &Url, status: StatusCode, headers: &HeaderMap) -> Retry {
        if status != StatusCode::UNAUTHORIZED {
            return Retry::Done;
        }
        if let Some(key) = self.source.dpop_key()
            && let Some(nonce) = dpop::resource_challenge(status, headers)
        {
            // NOTE: RFC 9449 §9: a nonce demanded by a resource refuses the proof, never the token, so the token stays and the resend's proof carries the nonce.
            key.remember(Role::Resource, url, nonce);
            return Retry::Resend;
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
