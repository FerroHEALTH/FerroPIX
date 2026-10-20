// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! How a data user authenticates each request to a Generic Function when a
//! fixed header cannot do it (feature `authorizer`, which `nvi` turns on).
//!
//! The IG authenticates a data user to a Generic Function on
//! GF-Authentication: the user holds an access token from GFI-004 and sends
//! it in an Authenticated Interaction (GFI-005), under the `DPoP` scheme with
//! a proof made for that one request (RFC 9449 §7.1). A proof binds the
//! request's method and URL, so it cannot ride in a default header of the
//! HTTP client. An [`Authorizer`] the caller supplies to a client, such as
//! the NVI client's `NviClient::with_authorizer`, gives the headers of each
//! request and reads each answer.
//!
//! The module holds no FHIR model, so a crate that only implements an
//! [`Authorizer`] compiles none. A client hands the authorizer the request
//! URL without its query and fragment, the `htu` of RFC 9449 §4.2, so the
//! NVI client never shows it the pseudonym its query carries.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use http::{HeaderMap, Method, StatusCode};
use url::Url;

/// The headers an [`Authorizer`] gives one request, once they are made.
pub type Authorized<'a> =
    Pin<Box<dyn Future<Output = Result<HeaderMap, AuthorizerError>> + Send + 'a>>;

/// The source of the headers, such as `Authorization` and `DPoP`, that
/// authenticate each request to a Generic Function service (the IG's
/// GFI-005).
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

    /// Reads the `status` and `headers` the service answered a request to
    /// `url` with, and returns whether the request is sent once more with
    /// new headers, as for a nonce the service demands (RFC 9449 §9).
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
