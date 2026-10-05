// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! GF-Localization through the national index, NVI (feature `nvi`).
//!
//! The data user side of the Localization Service search (the IG's
//! Localization page and the `nl-gf-localization-repository` capability
//! statement): a `search-type` on `DocumentReference` with the
//! `patient.identifier` and `type` search parameters, answered with a
//! `searchset` Bundle of localization records
//! (`nl-gf-localization-documentreference`). Each record states that the
//! care provider its `custodian` names by URA holds data of the record's type
//! for the patient its `subject` names by pseudonymised BSN.
//!
//! [`NviClient::localize`] asks for the records of type
//! [`PATIENT_DATA_TYPE`], the one type the IG fixes, and reads them into the
//! set of custodians, or a [`error::NviError`]. The Localization Service has
//! already checked the requester's access at each data holder before it
//! returns a record, so the answer is the custodians the service exposes to
//! this requester; it is never a statement that a custodian will release
//! data, which the data holder decides on every request.
//!
//! The pseudonym travels only in the request to the Localization Service,
//! which is the transaction's purpose: no error this module returns carries a
//! request URL, a pseudonym or the service's free text, and the types that
//! hold the pseudonym redact it in `Debug`.
//!
//! The data user authenticates through the transport, a default header of
//! the HTTP client, or an [`authorizer::Authorizer`] that gives each request
//! its own headers, as the `DPoP`-bound token of GF-Authentication needs
//! (the IG's GFI-005; RFC 9449 §7.1).
//!
//! # Examples
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use nl_generic_functions::identification::PseudoBsn;
//! use nl_generic_functions::nvi::NviClient;
//! use secrecy::SecretString;
//! use url::Url;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let http = reqwest::Client::builder()
//!     .redirect(reqwest::redirect::Policy::none())
//!     .build()?;
//! let client = NviClient::new(Url::parse("https://nvi.example.org/fhir/")?, http)?;
//! let patient = PseudoBsn::new(SecretString::from("pbsn-example-0001"))?;
//! let localization = client.localize(&patient, Duration::from_secs(2)).await?;
//! for custodian in localization.custodians() {
//!     let _ura = custodian.as_str();
//! }
//! # Ok(())
//! # }
//! # fn main() {
//! #     let _pending = run();
//! # }
//! ```

pub mod authorizer;
pub mod error;
mod request;
mod response;

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use http::header::{ACCEPT, CONTENT_TYPE};
use http::{HeaderMap, Method};
use url::Url;

use crate::identification::{PseudoBsn, Ura};
use authorizer::{Authorizer, Retry};
use error::{InvalidInput, Malformation, NviError};

/// The system of the localization record type the IG fixes (LOINC).
pub const LOINC_SYSTEM: &str = "http://loinc.org";

/// The localization record type the IG fixes for this version, LOINC
/// `55188-7` "Patient data Document" (the IG's Localization page, Data
/// models).
pub const PATIENT_DATA_TYPE: &str = "55188-7";

/// The media type the search asks for and reads (FHIR R4 JSON).
const FHIR_JSON: &str = "application/fhir+json";

/// The most `searchset` pages the client follows for one patient before it
/// refuses the answer as too large (no specification governs this: our own
/// design).
const PAGES: usize = 32;

/// What the Localization Service answered for one patient: the care
/// providers holding data of type [`PATIENT_DATA_TYPE`] for the patient
/// that the service exposes to this requester.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Localization {
    custodians: BTreeSet<Ura>,
}

impl Localization {
    /// Returns the custodians by URA, in order, each once.
    #[must_use]
    pub fn custodians(&self) -> &BTreeSet<Ura> {
        &self.custodians
    }

    /// Returns whether the service named no custodian.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.custodians.is_empty()
    }
}

/// A data user of the Localization Service bound to one NVI.
///
/// `Debug` shows the endpoint with its userinfo and query redacted, whether
/// an authorizer is set, and leaves out the HTTP client, whose default
/// headers may hold a credential.
#[derive(Clone)]
pub struct NviClient {
    endpoint: Url,
    http: reqwest::Client,
    authorizer: Option<Arc<dyn Authorizer>>,
}

impl NviClient {
    /// Creates a client for the Localization Service whose FHIR base URL is
    /// `base`.
    ///
    /// `http` carries the transport the caller chose: TLS, client
    /// certificates, default authorization headers. The request URL holds the
    /// pseudonym, so build `http` with `redirect::Policy::none()`: a client
    /// that follows redirects sends the pseudonym wherever the service points
    /// it, and a `3xx` is then an error like any other unexpected status.
    ///
    /// # Errors
    ///
    /// [`InvalidInput::Base`] when `base` is not an `http` or `https` URL
    /// without a query or a fragment.
    pub fn new(base: Url, http: reqwest::Client) -> Result<Self, InvalidInput> {
        Ok(Self {
            endpoint: request::endpoint(base)?,
            http,
            authorizer: None,
        })
    }

    /// Returns this client, asking `authorizer` for the headers of every
    /// request and handing it every answer (the IG's GFI-005).
    ///
    /// Build the HTTP client without a default `Authorization` header then:
    /// the authorizer's headers are added to it, never in its place.
    #[must_use]
    pub fn with_authorizer(mut self, authorizer: Arc<dyn Authorizer>) -> Self {
        self.authorizer = Some(authorizer);
        self
    }

    /// Returns the `[base]/DocumentReference` URL the client searches.
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Asks the Localization Service which care providers hold data of type
    /// [`PATIENT_DATA_TYPE`] for `patient`, following the answer's `next`
    /// links under the endpoint.
    ///
    /// `timeout` bounds the whole exchange, every page included, from
    /// connecting until the last answer is read; the time an authorizer
    /// takes to make its headers counts toward it.
    ///
    /// # Errors
    ///
    /// A [`NviError`] for every answer that is not a `searchset` of
    /// localization records about `patient`, for a request the authorizer
    /// cannot authenticate, and for a failure to get an answer at all.
    pub async fn localize(
        &self,
        patient: &PseudoBsn,
        timeout: Duration,
    ) -> Result<Localization, NviError> {
        let deadline = Instant::now().checked_add(timeout);
        let mut url = request::query(&self.endpoint, patient);
        let mut custodians = BTreeSet::new();
        for _page in 0..PAGES {
            let response = self.send(&url, deadline).await?;
            let status = response.status();
            let media = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let body = response::body(response).await?;
            let page = response::page(status, media.as_deref(), &body, patient)?;
            custodians.extend(page.custodians);
            match page.next {
                None => return Ok(Localization { custodians }),
                Some(next) => url = request::next(&self.endpoint, &next)?,
            }
        }
        Err(Malformation::TooManyPages { limit: PAGES }.into())
    }

    /// Sends the search `url` before `deadline`, with the authorizer's
    /// headers when there is one, once more when the authorizer asks for it.
    async fn send(
        &self,
        url: &Url,
        deadline: Option<Instant>,
    ) -> Result<reqwest::Response, NviError> {
        let target = request::target(url);
        let mut resent = false;
        loop {
            let headers = match &self.authorizer {
                Some(authorizer) => authorizer
                    .authorize(&Method::GET, &target)
                    .await
                    .map_err(NviError::Unauthenticated)?,
                None => HeaderMap::new(),
            };
            let remaining = deadline
                .and_then(|deadline| deadline.checked_duration_since(Instant::now()))
                .filter(|remaining| !remaining.is_zero())
                .ok_or(NviError::Timeout)?;
            let response = self
                .http
                .get(url.clone())
                .headers(headers)
                .header(ACCEPT, FHIR_JSON)
                .timeout(remaining)
                .send()
                .await
                .map_err(error::transport)?;
            let Some(authorizer) = &self.authorizer else {
                return Ok(response);
            };
            let retry = authorizer.answered(&target, response.status(), response.headers());
            if resent || retry == Retry::Done {
                return Ok(response);
            }
            resent = true;
        }
    }
}

impl fmt::Debug for NviClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NviClient")
            .field("endpoint", &request::Redacted(&self.endpoint))
            .field("authorizer", &self.authorizer.is_some())
            .finish_non_exhaustive()
    }
}
