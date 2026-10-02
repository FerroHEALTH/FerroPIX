// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! PDQm, Patient Demographics Query for Mobile (feature `pdqm`).
//!
//! The Patient Demographics Consumer side of ITI-78, Mobile Patient
//! Demographics Query (PDQm 3.2.0, ITI TF-2 §2:3.78): a search on the Patient
//! Demographics Supplier's `Patient` type with demographic criteria, answered
//! with a `searchset` Bundle of the matching Patients.
//!
//! [`PdqmClient::search`] sends the [`query::PatientQuery`] and reads the
//! answer into a [`matches::SearchResult`] or an [`error::PdqmError`];
//! [`PdqmClient::next_page`] follows the result set's paging
//! (§2:3.78.4.2.2.4). No match is a result with a `total` of `0`
//! (§2:3.78.4.1.3, Case 3); every failure, including a `404` that is not the
//! profile's unrecognised-domain answer (Case 4), is an error, so an outage or
//! a misrouted request never reads as no match.
//!
//! The criteria are directly identifying. They travel only to the Supplier,
//! which is the transaction's purpose, and in the body of a `POST` search
//! rather than in a URL, which the profile admits ("the Patient Demographics
//! Supplier SHALL support both GET and POST based searches", §2:3.78.4.1.2).
//! The query, every matched Patient and every page link redact their content
//! in `Debug`, none of them has `Display`, and no error this module returns
//! carries a value, a URL or the Supplier's free text.
//!
//! # Examples
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use ihe_iti::pdqm::PdqmClient;
//! use ihe_iti::pdqm::query::{DatePrefix, PatientQuery, StringMatch};
//! use secrecy::{ExposeSecret, SecretString};
//! use url::Url;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let http = reqwest::Client::builder()
//!     .redirect(reqwest::redirect::Policy::none())
//!     .build()?;
//! let client = PdqmClient::new(Url::parse("https://pdq.example.org/fhir/")?, http)?;
//! let query = PatientQuery::new()
//!     .family(&SecretString::from("Schmidt"), StringMatch::Exact)?
//!     .birthdate(DatePrefix::Eq, &SecretString::from("1923-07-25"))?;
//! let mut page = client.search(&query, Duration::from_secs(2)).await?;
//! loop {
//!     for matched in page.patients() {
//!         let _location = matched.full_url().expose_secret();
//!         let _deprecated = matched.patient().active.as_ref();
//!     }
//!     let Some(next) = page.next() else { break };
//!     page = client.next_page(next, Duration::from_secs(2)).await?;
//! }
//! # Ok(())
//! # }
//! # fn main() {
//! #     let _pending = run();
//! # }
//! ```

pub mod error;
pub mod matches;
pub mod query;
mod request;
mod response;

use std::time::Duration;

use http::header::{ACCEPT, CONTENT_TYPE};
use url::Url;

use error::{InvalidInput, PdqmError};
use matches::{Page, SearchResult};
use query::PatientQuery;

/// The media type ITI-78 asks for and reads (ITI TF-2 Appendix Z.6).
const FHIR_JSON: &str = "application/fhir+json";

/// A Patient Demographics Consumer bound to one Patient Demographics Supplier.
#[derive(Debug, Clone)]
pub struct PdqmClient {
    base: Url,
    endpoint: Url,
    http: reqwest::Client,
}

impl PdqmClient {
    /// Creates a client for the Supplier whose FHIR base URL is `base`.
    ///
    /// `http` carries the transport the caller chose: TLS, client
    /// certificates, default authorization headers (ITI TF-2 Appendix Z.8).
    /// Build it with `redirect::Policy::none()`: a client that follows a `307`
    /// sends the criteria wherever the Supplier points it, and a `3xx` is then
    /// an error like any other unexpected status.
    ///
    /// # Errors
    /// [`InvalidInput::Base`] when `base` is not an `http` or `https` URL
    /// without a query or a fragment.
    pub fn new(base: Url, http: reqwest::Client) -> Result<Self, InvalidInput> {
        let base = request::base(base)?;
        Ok(Self {
            endpoint: request::endpoint(base.clone())?,
            base,
            http,
        })
    }

    /// Returns the `[base]/Patient/_search` URL the client posts to
    /// (§2:3.78.4.1.2).
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Asks the Supplier for the first page of Patients that match `query`.
    ///
    /// `timeout` bounds the whole exchange, from connecting until the answer is
    /// read.
    ///
    /// # Errors
    /// A [`PdqmError`] for every answer that is not a `searchset` Bundle, and
    /// for a failure to get an answer at all.
    pub async fn search(
        &self,
        query: &PatientQuery,
        timeout: Duration,
    ) -> Result<SearchResult, PdqmError> {
        let response = self
            .http
            .post(self.endpoint.clone())
            .header(ACCEPT, FHIR_JSON)
            .header(CONTENT_TYPE, request::FORM)
            .body(query.form())
            .timeout(timeout)
            .send()
            .await
            .map_err(error::transport)?;
        self.answer(response, query.names_domain()).await
    }

    /// Asks the Supplier for the page `page` links to, the `next` link of an
    /// earlier page (FHIR R4 paging, <http://hl7.org/fhir/R4/http.html#paging>).
    ///
    /// # Errors
    /// [`PdqmError::ForeignPage`] when `page` is not on the Supplier's origin,
    /// which the client does not follow; otherwise as [`PdqmClient::search`].
    pub async fn next_page(
        &self,
        page: &Page,
        timeout: Duration,
    ) -> Result<SearchResult, PdqmError> {
        // NOTE: no specification governs this: our own design, so a Supplier's
        // link cannot send this client's credentials to another origin.
        if page.url().origin() != self.endpoint.origin() {
            return Err(PdqmError::ForeignPage);
        }
        let response = self
            .http
            .get(page.url().clone())
            .header(ACCEPT, FHIR_JSON)
            .timeout(timeout)
            .send()
            .await
            .map_err(error::transport)?;
        self.answer(response, false).await
    }

    async fn answer(
        &self,
        response: reqwest::Response,
        names_domain: bool,
    ) -> Result<SearchResult, PdqmError> {
        let status = response.status();
        let media = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response::body(response).await?;
        response::read(status, media.as_deref(), &body, &self.base, names_domain)
    }
}
