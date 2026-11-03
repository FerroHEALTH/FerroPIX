// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What every HTTP client of the crate does with one exchange, with features
//! `authorizer` or `soap`: the answer read up to a bound, and a failure to
//! get one read as the client's own error.
//!
//! A request URL may hold a patient identifier, a search criterion or the
//! credentials of a base, so a transport failure is kept without it. No
//! specification governs this: our own design.

use http::StatusCode;
use http::header::CONTENT_TYPE;

/// A client error that an exchange with no answer becomes.
pub trait Unanswered: Sized {
    /// No answer arrived within the timeout.
    fn timeout() -> Self;

    /// The request could not be sent or the answer could not be read;
    /// `source` carries no request URL.
    fn transport(source: reqwest::Error) -> Self;

    /// The failure `error`, with its request URL removed, as a timeout when
    /// it is one.
    #[must_use]
    fn failed(error: reqwest::Error) -> Self {
        let error = error.without_url();
        if error.is_timeout() {
            Self::timeout()
        } else {
            Self::transport(error)
        }
    }
}

/// An answer, read whole.
#[derive(Debug)]
pub struct Answer {
    /// The HTTP status.
    pub status: StatusCode,
    /// The `Content-Type` header, when it is text.
    pub media: Option<String>,
    /// The body.
    pub body: Vec<u8>,
}

/// Reads `response` whole, refusing a body longer than `limit` bytes with
/// the error `too_large` makes of the answer's status.
///
/// # Errors
///
/// The error `too_large` makes for a body longer than `limit`, and
/// [`Unanswered::failed`] when the body cannot be read.
pub async fn read<E: Unanswered>(
    mut response: reqwest::Response,
    limit: usize,
    too_large: impl FnOnce(StatusCode) -> E,
) -> Result<Answer, E> {
    let status = response.status();
    let media = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(E::failed)? {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(too_large(status));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Answer {
        status,
        media,
        body,
    })
}
