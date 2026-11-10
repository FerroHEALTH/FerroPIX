// SPDX-FileCopyrightText: Cadasto B.V.
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
//! The client is built from `ferrohealth-smart`'s [`OutboundClient`], so the
//! data user authenticates through the transport its endpoint table names,
//! HTTP Basic, or a client credentials token in each request's
//! `Authorization` header, with a proof of each request for a `DPoP`-bound
//! token as GF-Authentication needs (the IG's GFI-005; RFC 9449 §7.1).
//!
//! # Examples
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use ferrohealth_smart::client::outbound::OutboundClient;
//! use ferrohealth_smart::config::TlsBuilder;
//! use ferrohealth_smart::config::endpoint::EndpointSection;
//! use nl_generic_functions::identification::PseudoBsn;
//! use nl_generic_functions::nvi::NviClient;
//! use secrecy::SecretString;
//!
//! # fn configured() -> (EndpointSection, &'static dyn TlsBuilder) {
//! #     unimplemented!("the product's configuration and its reader of TLS material")
//! # }
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! // The `[nvi.endpoint]` table, whose `url` is the service's FHIR base.
//! let (endpoint, tls) = configured();
//! let client = NviClient::new(OutboundClient::build(&endpoint, "nvi.endpoint", tls)?)?;
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

pub mod error;
mod request;
mod response;

use std::collections::BTreeSet;
use std::fmt;
use std::time::{Duration, Instant};

use ferrohealth_smart::client::outbound::OutboundClient;
use http::Method;
use http::header::ACCEPT;
use url::Url;

use crate::identification::{PseudoBsn, Ura};
use error::{InvalidInput, Malformation, NviError};
use ihe_iti::exchange;
use ihe_iti::outbound;

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
/// `Debug` shows the endpoint with its userinfo and query redacted and
/// whether the client carries a credential, never the credential itself.
#[derive(Clone)]
pub struct NviClient {
    endpoint: Url,
    client: OutboundClient,
}

impl NviClient {
    /// Creates a client for the Localization Service whose FHIR base URL is
    /// `client`'s URL, reached through `client`.
    ///
    /// `client` carries the transport its endpoint table names: TLS, client
    /// certificates, the credential every request carries and the retry
    /// table. The request URL holds the pseudonym, and `client` follows no
    /// redirect, so the pseudonym goes nowhere the base does not name and a
    /// `3xx` is an error like any other unexpected status.
    ///
    /// # Errors
    ///
    /// [`InvalidInput::Base`] when the URL is not an `http` or `https` URL
    /// without a query or a fragment.
    pub fn new(client: OutboundClient) -> Result<Self, InvalidInput> {
        Ok(Self {
            endpoint: request::endpoint(client.url().clone())?,
            client,
        })
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
    /// connecting until the last answer is read; the time a token takes to
    /// obtain counts toward it.
    ///
    /// # Errors
    ///
    /// A [`NviError`] for every answer that is not a `searchset` of
    /// localization records about `patient`, for a request no credential
    /// could be had for, and for a failure to get an answer at all.
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
            let answer = exchange::read(response, response::LIMIT, response::too_large).await?;
            let page = response::page(
                answer.status,
                answer.media.as_deref(),
                &answer.body,
                patient,
            )?;
            custodians.extend(page.custodians);
            match page.next {
                None => return Ok(Localization { custodians }),
                Some(next) => url = request::next(&self.endpoint, &next)?,
            }
        }
        Err(Malformation::TooManyPages { limit: PAGES }.into())
    }

    /// Sends the search `url` before `deadline`, through the outbound client.
    async fn send(
        &self,
        url: &Url,
        deadline: Option<Instant>,
    ) -> Result<reqwest::Response, NviError> {
        let remaining = deadline
            .and_then(|deadline| deadline.checked_duration_since(Instant::now()))
            .filter(|remaining| !remaining.is_zero())
            .ok_or(NviError::Timeout)?;
        outbound::send(&self.client, &Method::GET, url, remaining, |request| {
            request.header(ACCEPT, FHIR_JSON)
        })
        .await
    }
}

impl fmt::Debug for NviClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NviClient")
            .field("endpoint", &request::Redacted(&self.endpoint))
            .field("authorization", &self.client.authorization().is_some())
            .finish_non_exhaustive()
    }
}
