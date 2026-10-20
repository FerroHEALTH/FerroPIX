// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ATX: FHIR Feed Option of ITI-20 (feature `balp`).
//!
//! Send Audit Resource Request is a FHIR `create` of one `AuditEvent` at an
//! Audit Record Repository (the `RESTful` ATNA supplement, ITI TF-2
//! §3.20.4.2), answered by Send Audit Resource Response (§3.20.4.3).
//!
//! [`FeedRepository::send`] posts one stored record to `[base]/AuditEvent` as
//! FHIR JSON (ITI TF-2 Appendix Z.6; FHIR R4 `create`,
//! <http://hl7.org/fhir/R4/http.html#create>). A `2xx` is the repository's
//! success (§3.20.4.3.2). §3.20.4.3.3 leaves a failure to the client: a
//! `4xx` other than `408` and `429` is [`FeedError::Rejected`], which no
//! retry of the same record changes, and every other failure is
//! [`FeedError::Unavailable`], which a later attempt may get past.
//!
//! The record names a patient, so the repository is reached over TLS unless a
//! development caller asks otherwise, no redirect is followed, and no error
//! carries the record or the repository's answer body.

use std::fmt;
use std::time::Duration;

use http::StatusCode;
use http::header::CONTENT_TYPE;
use secrecy::{ExposeSecret, SecretSlice};
use url::Url;

use crate::redact::RedactedUrl;

/// The media type a record is sent as (ITI TF-2 Appendix Z.6).
const FHIR_JSON: &str = "application/fhir+json";

/// Why a repository's FHIR base was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FeedAddressError {
    /// The base is not an `https` URL, or an `http` one for development,
    /// without a query or a fragment.
    #[error("the audit repository FHIR base is not an https URL without a query or a fragment")]
    Base,
    /// The base is `http` and the caller did not ask for clear text.
    #[error("the audit repository FHIR base is not https")]
    Cleartext,
}

/// Why a record was not accepted.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FeedError {
    /// The repository refused the record with a `4xx` other than `408` or
    /// `429`: sending it again would be refused again.
    #[error("the audit repository refused the AuditEvent with {status}")]
    Rejected {
        /// The HTTP status.
        status: StatusCode,
    },
    /// The repository answered a status that may pass: a `5xx`, `408`,
    /// `429`, or a status the create interaction gives no meaning to.
    #[error("the audit repository answered {status}")]
    Unavailable {
        /// The HTTP status.
        status: StatusCode,
    },
    /// The request could not be sent, or no answer arrived within the
    /// timeout.
    #[error("the audit repository could not be reached")]
    Transport(#[source] reqwest::Error),
}

/// An Audit Record Repository's FHIR Feed endpoint.
///
/// `Debug` shows the endpoint without its userinfo and leaves out the HTTP
/// client, whose default headers may hold a credential.
#[derive(Clone)]
pub struct FeedRepository {
    endpoint: Url,
    http: reqwest::Client,
    timeout: Duration,
}

impl FeedRepository {
    /// The repository whose FHIR base is `base`, reached over `https`
    /// through `http`, each request bounded by `timeout`.
    ///
    /// Build `http` with `redirect::Policy::none()`: a record names a
    /// patient, and a client that follows redirects sends it wherever the
    /// repository points.
    ///
    /// # Errors
    ///
    /// [`FeedAddressError::Base`] for a base that is no `http(s)` URL
    /// without a query or a fragment, and [`FeedAddressError::Cleartext`]
    /// for an `http` one.
    pub fn new(
        base: Url,
        http: reqwest::Client,
        timeout: Duration,
    ) -> Result<Self, FeedAddressError> {
        if base.scheme() != "https" {
            return if base.scheme() == "http" {
                Err(FeedAddressError::Cleartext)
            } else {
                Err(FeedAddressError::Base)
            };
        }
        Self::any_scheme(base, http, timeout)
    }

    /// The repository at `base` over `http` or `https`, for a development
    /// deployment whose records may cross a network in clear text.
    ///
    /// # Errors
    ///
    /// [`FeedAddressError::Base`] for a base that is no `http(s)` URL
    /// without a query or a fragment.
    pub fn cleartext_for_development(
        base: Url,
        http: reqwest::Client,
        timeout: Duration,
    ) -> Result<Self, FeedAddressError> {
        Self::any_scheme(base, http, timeout)
    }

    fn any_scheme(
        base: Url,
        http: reqwest::Client,
        timeout: Duration,
    ) -> Result<Self, FeedAddressError> {
        let endpoint =
            crate::search::under_base(base, "AuditEvent").ok_or(FeedAddressError::Base)?;
        Ok(Self {
            endpoint,
            http,
            timeout,
        })
    }

    /// The `[base]/AuditEvent` URL records are posted to.
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Posts `record`, one `AuditEvent` as FHIR JSON (§3.20.4.2.2).
    ///
    /// # Errors
    ///
    /// A [`FeedError`] for every answer but a `2xx`, and for no answer.
    pub async fn send(&self, record: &SecretSlice<u8>) -> Result<(), FeedError> {
        let answer = self
            .http
            .post(self.endpoint.clone())
            .header(CONTENT_TYPE, FHIR_JSON)
            .body(record.expose_secret().to_vec())
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|error| FeedError::Transport(error.without_url()))?;
        let status = answer.status();
        if status.is_success() {
            return Ok(());
        }
        if status.is_client_error()
            && status != StatusCode::REQUEST_TIMEOUT
            && status != StatusCode::TOO_MANY_REQUESTS
        {
            return Err(FeedError::Rejected { status });
        }
        Err(FeedError::Unavailable { status })
    }
}

impl fmt::Debug for FeedRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FeedRepository")
            .field("endpoint", &RedactedUrl(self.endpoint.as_str()))
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}
