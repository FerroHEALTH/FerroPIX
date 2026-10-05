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
//! The same client is the Consumer side of ITI-119, Patient Demographics Match
//! (ITI TF-2 §2:3.119): [`PdqmClient::match_patient`] posts the
//! [`input::MatchInput`] to `[base]/Patient/$match` and reads the answer into a
//! [`matches::MatchResult`], every matched Patient with its score and
//! `match-grade` (§2:3.119.4.2.2.4). No match is a result with no Patient
//! (§2:3.119.4.1.3, Cases 4, 5 and 7).
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
//! use ihe_iti::user::OnBehalfOf;
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
//! let on_behalf = OnBehalfOf::System;
//! let mut page = client.search(&query, &on_behalf, Duration::from_secs(2)).await?;
//! loop {
//!     for matched in page.patients() {
//!         let _location = matched.full_url().expose_secret();
//!         let _deprecated = matched.patient().active.as_ref();
//!     }
//!     let Some(next) = page.next() else { break };
//!     page = client
//!         .next_page(next, &on_behalf, Duration::from_secs(2))
//!         .await?;
//! }
//! # Ok(())
//! # }
//! # fn main() {
//! #     let _pending = run();
//! # }
//! ```

#[cfg(feature = "balp")]
pub mod audit;
pub mod error;
pub mod input;
pub mod matches;
pub mod query;
mod request;
mod response;

use std::fmt;
use std::time::Duration;

use http::header::{ACCEPT, CONTENT_TYPE};
use url::Url;

use crate::redact::RedactedUrl;
use crate::user::OnBehalfOf;
use error::{InvalidInput, PdqmError};
use input::MatchInput;
use matches::{MatchResult, Page, SearchResult};
use query::PatientQuery;

/// The media type ITI-78 asks for and reads (ITI TF-2 Appendix Z.6).
const FHIR_JSON: &str = "application/fhir+json";

/// A Patient Demographics Consumer bound to one Patient Demographics Supplier.
///
/// `Debug` shows the base and the endpoint with their userinfo replaced by
/// `***`, and leaves out the HTTP client, whose default headers may hold a
/// credential.
#[derive(Clone)]
pub struct PdqmClient {
    base: Url,
    endpoint: Url,
    match_endpoint: Url,
    http: reqwest::Client,
    #[cfg(feature = "balp")]
    audit: Option<std::sync::Arc<dyn crate::balp::AuditRecorder>>,
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
            match_endpoint: request::match_endpoint(base.clone())?,
            base,
            http,
            #[cfg(feature = "balp")]
            audit: None,
        })
    }

    /// This client, recording the audit record of every search, every page
    /// and every match through `recorder` (§2:3.78.5.1, §2:3.119.5.1.1,
    /// feature `balp`).
    ///
    /// An audited client records each request before it returns, whatever
    /// its outcome; one whose record the recorder does not accept fails with
    /// [`PdqmError::Audit`], and its answer is not used.
    #[cfg(feature = "balp")]
    #[must_use]
    pub fn audited(mut self, recorder: std::sync::Arc<dyn crate::balp::AuditRecorder>) -> Self {
        self.audit = Some(recorder);
        self
    }

    /// Records the request `request`, when the client is audited, and
    /// returns `result` unless the record was refused, or was not accepted
    /// by `deadline` for an answer that would be used
    /// ([`crate::recording`]).
    #[cfg(feature = "balp")]
    async fn audit(
        &self,
        request: impl FnOnce() -> secrecy::SecretString,
        on_behalf: &OnBehalfOf,
        result: Result<SearchResult, PdqmError>,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<SearchResult, PdqmError> {
        use crate::recording::{Late, Recorded, within};
        if let Some(recorder) = &self.audit {
            let exchange = audit::exchange(&self.base, request(), on_behalf, &result);
            // NOTE: PDQm §2:3.78.5.1 makes the audit record part of the query, so
            // an answer whose record was refused is not used.
            match within(deadline, recorder.record(exchange)).await {
                Recorded::Refused(error) => return Err(PdqmError::Audit(error)),
                Recorded::Late if result.is_ok() => {
                    return Err(PdqmError::Audit(crate::balp::AuditError(Box::new(Late))));
                }
                Recorded::Accepted | Recorded::Late => {}
            }
        }
        result
    }

    /// Returns the `[base]/Patient/_search` URL the client posts to
    /// (§2:3.78.4.1.2).
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Returns the `[base]/Patient/$match` URL the client posts a match to
    /// (§2:3.119.4.1.2).
    #[must_use]
    pub fn match_endpoint(&self) -> &Url {
        &self.match_endpoint
    }

    /// Asks the Supplier for the Patients that match `input` (ITI-119), on
    /// behalf of `on_behalf`, whom an audited client's record names.
    ///
    /// `timeout` bounds the whole exchange, from connecting until the answer is
    /// read, and an audited client's record of it.
    ///
    /// # Errors
    /// [`PdqmError::Unwritable`] before anything is sent when the input
    /// cannot be written as JSON, and a [`PdqmError`] for every answer that is
    /// not a Match Output Bundle and for a failure to get an answer at all.
    pub async fn match_patient(
        &self,
        input: &MatchInput,
        #[cfg_attr(
            not(feature = "balp"),
            expect(
                unused_variables,
                reason = "only an audited client names whom it acted for"
            )
        )]
        on_behalf: &OnBehalfOf,
        timeout: Duration,
    ) -> Result<MatchResult, PdqmError> {
        #[cfg(feature = "balp")]
        let deadline = crate::recording::deadline(timeout);
        let body = secrecy::SecretString::from(
            serde_json::to_string(&input.parameters()).map_err(PdqmError::Unwritable)?,
        );
        let result = self.post_match(&body, timeout).await;
        #[cfg(feature = "balp")]
        let result = self
            .audit_match((input, &body), on_behalf, result, deadline)
            .await;
        result
    }

    /// Records the match of `input`, sent as `body`, when the client is
    /// audited, and returns `result` unless the record was refused, or was
    /// not accepted by `deadline` for an answer that would be used
    /// ([`crate::recording`]).
    #[cfg(feature = "balp")]
    async fn audit_match(
        &self,
        (input, body): (&MatchInput, &secrecy::SecretString),
        on_behalf: &OnBehalfOf,
        result: Result<MatchResult, PdqmError>,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<MatchResult, PdqmError> {
        use crate::recording::{Late, Recorded, within};
        use secrecy::ExposeSecret as _;
        if let Some(recorder) = &self.audit {
            // NOTE: BALP Query records the raw request; a POST operation is its
            // request line, media type and body, as for the ITI-78 POST search.
            let request = secrecy::SecretString::from(format!(
                "POST {}\nContent-Type: {FHIR_JSON}\n\n{}",
                crate::balp::request_text(&self.match_endpoint).expose_secret(),
                body.expose_secret()
            ));
            let exchange = audit::match_exchange(
                &self.base,
                (request, input.sole_identifier()),
                on_behalf,
                &result,
            );
            // NOTE: PDQm §2:3.119.5.1.1 makes the audit record part of the match, so
            // an answer whose record was refused is not used.
            match within(deadline, recorder.record(exchange)).await {
                Recorded::Refused(error) => return Err(PdqmError::Audit(error)),
                Recorded::Late if result.is_ok() => {
                    return Err(PdqmError::Audit(crate::balp::AuditError(Box::new(Late))));
                }
                Recorded::Accepted | Recorded::Late => {}
            }
        }
        result
    }

    async fn post_match(
        &self,
        body: &secrecy::SecretString,
        timeout: Duration,
    ) -> Result<MatchResult, PdqmError> {
        use secrecy::ExposeSecret as _;
        let body = body.expose_secret().as_bytes().to_vec();
        let response = self
            .http
            .post(self.match_endpoint.clone())
            .header(ACCEPT, FHIR_JSON)
            .header(CONTENT_TYPE, FHIR_JSON)
            .body(body)
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
        response::read_match(status, media.as_deref(), &body)
    }

    /// Asks the Supplier for the first page of Patients that match `query`,
    /// on behalf of `on_behalf`, whom an audited client's record names.
    ///
    /// `timeout` bounds the whole exchange, from connecting until the answer is
    /// read, and an audited client's record of it.
    ///
    /// # Errors
    /// A [`PdqmError`] for every answer that is not a `searchset` Bundle, and
    /// for a failure to get an answer at all.
    pub async fn search(
        &self,
        query: &PatientQuery,
        #[cfg_attr(
            not(feature = "balp"),
            expect(
                unused_variables,
                reason = "only an audited client names whom it acted for"
            )
        )]
        on_behalf: &OnBehalfOf,
        timeout: Duration,
    ) -> Result<SearchResult, PdqmError> {
        #[cfg(feature = "balp")]
        let deadline = crate::recording::deadline(timeout);
        let result = self.post(query, timeout).await;
        #[cfg(feature = "balp")]
        let result = self
            .audit(
                || {
                    // NOTE: BALP Query records the raw request; a POST search is its
                    // request line, media type and form body, as BALP's own example writes.
                    secrecy::SecretString::from(format!(
                        "POST {}\nContent-Type: {}\n\n{}",
                        secrecy::ExposeSecret::expose_secret(&crate::balp::request_text(
                            &self.endpoint
                        )),
                        request::FORM,
                        query.form()
                    ))
                },
                on_behalf,
                result,
                deadline,
            )
            .await;
        result
    }

    async fn post(
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
    /// earlier page (FHIR R4 paging, <http://hl7.org/fhir/R4/http.html#paging>),
    /// on behalf of `on_behalf`, whom an audited client's record names.
    ///
    /// # Errors
    /// [`PdqmError::ForeignPage`] when `page` is not on the Supplier's origin,
    /// which the client does not follow; otherwise as [`PdqmClient::search`].
    pub async fn next_page(
        &self,
        page: &Page,
        #[cfg_attr(
            not(feature = "balp"),
            expect(
                unused_variables,
                reason = "only an audited client names whom it acted for"
            )
        )]
        on_behalf: &OnBehalfOf,
        timeout: Duration,
    ) -> Result<SearchResult, PdqmError> {
        // NOTE: no specification governs this: our own design, so a Supplier's
        // link cannot send this client's credentials to another origin.
        if page.url().origin() != self.endpoint.origin() {
            return Err(PdqmError::ForeignPage);
        }
        #[cfg(feature = "balp")]
        let deadline = crate::recording::deadline(timeout);
        let result = self.get(page, timeout).await;
        #[cfg(feature = "balp")]
        let result = self
            .audit(
                || crate::balp::request_text(page.url()),
                on_behalf,
                result,
                deadline,
            )
            .await;
        result
    }

    async fn get(&self, page: &Page, timeout: Duration) -> Result<SearchResult, PdqmError> {
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

impl fmt::Debug for PdqmClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PdqmClient")
            .field("base", &RedactedUrl(self.base.as_str()))
            .field("endpoint", &RedactedUrl(self.endpoint.as_str()))
            .finish_non_exhaustive()
    }
}
