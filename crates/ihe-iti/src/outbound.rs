// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! How every HTTP client of the crate sends a request (feature `outbound`,
//! which `pixm`, `pdqm`, `mcsd`, `pmir` and `soap` turn on), with no FHIR
//! model.
//!
//! Each client is built from `ferrohealth-smart`'s [`OutboundClient`], the
//! one outbound client of the FerroHEALTH products, which its endpoint table
//! builds: the TLS, the timeouts, no redirect, the `retry` table, and the
//! credential every request carries. A client credentials token goes in the
//! `Authorization` header (IUA ITI-72 §3.72.4.2), with a `DPoP` proof of
//! each request for a bound token (RFC 9449 §7.1); after a `401` the refused
//! token is dropped and the request sent once more with a new one
//! (§3.72.4.3), and a resource's `DPoP` nonce is answered once with the
//! token kept (RFC 9449 §9).
//!
//! [`send`] bounds a whole exchange by the caller's timeout: every try, the
//! waits between them, and the reading of the answer the last try gets.

use std::time::{Duration, Instant};

use ferrohealth_smart::client::outbound::{OutboundClient, SendError};
use http::Method;
use url::Url;

use crate::exchange::Unanswered;

/// A client error that a request [`send`] could not send becomes: the
/// failures of [`Unanswered`], and the credential that could not be had.
pub trait Unauthenticated: Unanswered {
    /// No token could be obtained, or a refused one renewed (IUA ITI-72
    /// §3.72.4.3); `source` carries no request URL.
    fn unauthenticated(source: SendError) -> Self;
}

/// Sends a `method` request to `url` through `client`, shaped by `prepare`,
/// within `timeout`.
///
/// `prepare` builds a request that is safe to repeat, since `client` tries
/// it again as its `retry` table allows. `timeout` bounds every try, the
/// waits between them and the answer's body together.
///
/// # Errors
///
/// [`Unanswered::timeout`] when `timeout` runs out before an answer,
/// [`Unanswered::failed`] when the last try gets no answer, and
/// [`Unauthenticated::unauthenticated`] when no credential could be had.
pub async fn send<E, F>(
    client: &OutboundClient,
    method: &Method,
    url: &Url,
    timeout: Duration,
    prepare: F,
) -> Result<reqwest::Response, E>
where
    E: Unauthenticated,
    F: Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
{
    let deadline = Instant::now().checked_add(timeout);
    let bounded = |request: reqwest::RequestBuilder| {
        let remaining = deadline.map_or(timeout, |deadline| {
            deadline.saturating_duration_since(Instant::now())
        });
        prepare(request).timeout(remaining)
    };
    match tokio::time::timeout(timeout, client.send(method, url, bounded)).await {
        Err(_elapsed) => Err(E::timeout()),
        Ok(Ok(response)) => Ok(response),
        Ok(Err(SendError::Transport(error))) => Err(E::failed(error)),
        Ok(Err(other)) => Err(E::unauthenticated(other)),
    }
}
