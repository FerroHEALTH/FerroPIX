// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! How a client authenticates each request to an IHE FHIR server when a
//! fixed header cannot do it (features `pixm`, `pdqm`, `mcsd`, `pmir`).
//!
//! A client that holds an OAuth 2.0 access token incorporates it in the
//! `Authorization` header of each request (IUA ITI-72 §3.72.4.2), and a
//! token expires: the client obtains a new one before it does, and after a
//! `401` obtains a new one before it retries, never retrying with the token
//! that was refused (§3.72.4.3). A default header of the HTTP client cannot
//! change between requests, so an [`Authorizer`] the caller supplies gives
//! the headers of each request and reads each answer. The PIXm, PDQm, mCSD
//! and PMIR clients each take one with `with_authorizer`.
//!
//! The authorizer is handed the request URL without its query and fragment,
//! so a patient identifier or a search criterion in the query never reaches
//! it. A client sends a request at most twice: once, and once more when the
//! authorizer asks for it after the first answer.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use http::{HeaderMap, Method, StatusCode};
use url::Url;

/// The headers an [`Authorizer`] gives one request, once they are made.
pub type Authorized<'a> =
    Pin<Box<dyn Future<Output = Result<HeaderMap, AuthorizerError>> + Send + 'a>>;

/// The source of the headers that authenticate each request to an IHE FHIR
/// server, such as the `Authorization` header that carries an access token
/// (IUA ITI-72 §3.72.4.2).
pub trait Authorizer: Send + Sync + fmt::Debug {
    /// Returns the headers that authenticate a request with `method` to
    /// `url`, which is the request URL without its query and fragment.
    ///
    /// Each header that carries a credential is marked sensitive.
    ///
    /// # Errors
    ///
    /// An [`AuthorizerError`] when no credential can be made; the request is
    /// then not sent.
    fn authorize<'a>(&'a self, method: &'a Method, url: &'a Url) -> Authorized<'a>;

    /// Reads the `status` and `headers` the server answered a request to
    /// `url` with, and returns whether the request is sent once more with
    /// new headers, as after a `401` that refused the token (IUA ITI-72
    /// §3.72.4.3).
    ///
    /// The client sends a request at most twice, whatever this returns.
    fn answered(&self, url: &Url, status: StatusCode, headers: &HeaderMap) -> Retry;
}

/// What the client does after an answer, as the [`Authorizer`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    /// The answer stands.
    Done,
    /// The request is sent once more, with new headers.
    Resend,
}

/// An [`Authorizer`] that could not make the headers of a request.
///
/// The source an authorizer reports never carries a credential.
#[derive(Debug, thiserror::Error)]
#[error("the authorizer could not make the headers of the request")]
pub struct AuthorizerError {
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
}

impl AuthorizerError {
    /// The failure `source` an authorizer reported.
    #[must_use]
    pub fn new(source: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self {
            source: source.into(),
        }
    }
}

/// Why [`send`] produced no answer.
#[derive(Debug)]
pub(crate) enum Unsent {
    /// The authorizer made no headers, so nothing was sent.
    Unauthenticated(AuthorizerError),
    /// The timeout ran out before the request could be sent once more.
    Timeout,
    /// The request could not be sent, or no answer arrived.
    Transport(reqwest::Error),
}

/// Sends `request` over `http` within `timeout`, with the headers of
/// `authorizer` when there is one, once more when the authorizer asks for
/// it after the first answer and the request can be repeated.
///
/// `timeout` bounds every send together, the time the authorizer takes
/// included; a request built with a body that cannot be repeated is sent
/// once.
pub(crate) async fn send(
    http: &reqwest::Client,
    authorizer: Option<&dyn Authorizer>,
    mut request: reqwest::Request,
    timeout: Duration,
) -> Result<reqwest::Response, Unsent> {
    let Some(authorizer) = authorizer else {
        *request.timeout_mut() = Some(timeout);
        return http.execute(request).await.map_err(Unsent::Transport);
    };
    let deadline = Instant::now().checked_add(timeout);
    let mut target = request.url().clone();
    target.set_query(None);
    target.set_fragment(None);
    let mut resent = false;
    loop {
        let spare = if resent { None } else { request.try_clone() };
        let headers = authorizer
            .authorize(request.method(), &target)
            .await
            .map_err(Unsent::Unauthenticated)?;
        request.headers_mut().extend(headers);
        let remaining = deadline
            .and_then(|deadline| deadline.checked_duration_since(Instant::now()))
            .filter(|remaining| !remaining.is_zero())
            .ok_or(Unsent::Timeout)?;
        *request.timeout_mut() = Some(remaining);
        let response = http.execute(request).await.map_err(Unsent::Transport)?;
        let retry = authorizer.answered(&target, response.status(), response.headers());
        match (retry, spare) {
            (Retry::Resend, Some(spare)) => {
                request = spare;
                resent = true;
            }
            _ => return Ok(response),
        }
    }
}
