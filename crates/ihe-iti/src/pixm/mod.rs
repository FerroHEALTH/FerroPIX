// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! PIXm, Patient Identifier Cross-reference for Mobile (feature `pixm`).
//!
//! The Patient Identifier Cross-reference Consumer side of ITI-83, Get
//! Corresponding Identifiers (PIXm 3.1.0, ITI TF-2 §2:3.83): the `$ihe-pix`
//! operation on the Patient Identifier Cross-reference Manager's `Patient`
//! type, asked with one `sourceIdentifier` (an assigning authority and an
//! identifier value) and zero or more `targetSystem` domains, answered with the
//! identifiers the other domains hold for the same patient.
//!
//! [`PixmClient::cross_reference`] makes the HTTP `GET` and reads the answer
//! into an [`identifier::CrossReference`] or an [`error::PixmError`]. The two
//! answers the profile defines for an unknown patient (§2:3.83.4.2.2.2 and
//! §2:3.83.4.2.2.5) are `CrossReference::SourceNotFound`; every other failure,
//! including a `404` that carries no `not-found` issue, is an error, so an
//! outage or a misrouted request never reads as a patient with no identifiers.
//!
//! The source identifier is a directly identifying value. It travels only in
//! the request to the PIX Manager, which is the transaction's purpose: the
//! types that carry it and every identifier the Manager returns redact it in
//! `Debug`, none of them has `Display`, and no error this module returns
//! carries a value, a request URL or the Manager's free text.
//!
//! # Examples
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use ihe_iti::pixm::PixmClient;
//! use ihe_iti::pixm::identifier::{CrossReference, SourceIdentifier, TargetSystem};
//! use secrecy::{ExposeSecret, SecretString};
//! use url::Url;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let http = reqwest::Client::builder()
//!     .redirect(reqwest::redirect::Policy::none())
//!     .build()?;
//! let client = PixmClient::new(Url::parse("https://pix.example.org/fhir/")?, http)?;
//! let source = SourceIdentifier::new(
//!     "urn:oid:1.3.6.1.4.1.21367.13.20.1000",
//!     SecretString::from("IHERED-994"),
//! )?;
//! let targets = [TargetSystem::new("urn:oid:1.3.6.1.4.1.21367.13.20.3000")?];
//! match client.cross_reference(&source, &targets, Duration::from_secs(2)).await? {
//!     CrossReference::Matched(found) => {
//!         for identifier in found.identifiers() {
//!             let _value = identifier.value().expose_secret();
//!         }
//!     }
//!     CrossReference::SourceNotFound => {}
//!     _ => {}
//! }
//! # Ok(())
//! # }
//! # fn main() {
//! #     let _pending = run();
//! # }
//! ```

pub mod error;
pub mod identifier;
mod request;
mod response;

use std::fmt;
use std::time::Duration;

use http::header::{ACCEPT, CONTENT_TYPE};
use url::Url;

use crate::redact::RedactedUrl;
use error::{InvalidInput, PixmError};
use identifier::{CrossReference, SourceIdentifier, TargetSystem};

/// The media type ITI-83 asks for and reads (ITI TF-2 Appendix Z.6).
const FHIR_JSON: &str = "application/fhir+json";

/// A Patient Identifier Cross-reference Consumer bound to one PIX Manager.
///
/// `Debug` shows the endpoint with its userinfo replaced by `***`, and leaves
/// out the HTTP client, whose default headers may hold a credential.
#[derive(Clone)]
pub struct PixmClient {
    endpoint: Url,
    http: reqwest::Client,
}

impl PixmClient {
    /// Creates a client for the PIX Manager whose FHIR base URL is `base`.
    ///
    /// `http` carries the transport the caller chose: TLS, client
    /// certificates, default authorization headers (ITI TF-2 Appendix Z.8).
    /// The request URL holds the source identifier, so build `http` with
    /// `redirect::Policy::none()`: a client that follows redirects sends the
    /// identifier wherever the Manager points it, and a `3xx` is then an
    /// error like any other unexpected status.
    ///
    /// # Errors
    /// [`InvalidInput::Base`] when `base` is not an `http` or `https` URL
    /// without a query or a fragment.
    pub fn new(base: Url, http: reqwest::Client) -> Result<Self, InvalidInput> {
        Ok(Self {
            endpoint: request::endpoint(base)?,
            http,
        })
    }

    /// Returns the `[base]/Patient/$ihe-pix` URL the client asks
    /// (§2:3.83.4.1.2).
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Asks the PIX Manager for the identifiers the `targets` domains hold for
    /// the patient `source` names, or every domain's when `targets` is empty
    /// (§2:3.83.4.1.2.2).
    ///
    /// `timeout` bounds the whole exchange, from connecting until the answer is
    /// read.
    ///
    /// # Errors
    /// A [`PixmError`] for every answer that is neither a cross-reference nor
    /// one of the profile's two not-found answers, and for a failure to get an
    /// answer at all.
    pub async fn cross_reference(
        &self,
        source: &SourceIdentifier,
        targets: &[TargetSystem],
        timeout: Duration,
    ) -> Result<CrossReference, PixmError> {
        let response = self
            .http
            .get(request::query(&self.endpoint, source, targets))
            .header(ACCEPT, FHIR_JSON)
            .timeout(timeout)
            .send()
            .await
            .map_err(error::transport)?;
        let status = response.status();
        let media = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response::body(response).await?;
        response::read(status, media.as_deref(), &body, source, targets)
    }
}

impl fmt::Debug for PixmClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PixmClient")
            .field("endpoint", &RedactedUrl(self.endpoint.as_str()))
            .finish_non_exhaustive()
    }
}
