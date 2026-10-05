// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! XCPD, Cross-Community Patient Discovery (feature `xcpd`).
//!
//! The Initiating Gateway side of ITI-55, Cross Gateway Patient Discovery
//! (ITI TF-2 §3.55, Revision 20.1): a `PRPA_IN201305UV02` query sent to a
//! Responding Gateway in a SOAP 1.2 envelope with its WS-Addressing headers
//! (Appendix V), answered synchronously with a `PRPA_IN201306UV02` that
//! names, per matching record, the community that holds it.
//!
//! [`XcpdClient::discover`] asks one responding gateway by the shared patient
//! identifier (§3.55.1, the identifier mode; no demographics are sent) and
//! reads the answer into a [`discovery::Discovery`] for Cases 1 to 4 of
//! §3.55.4.2.3, or an [`error::XcpdError`] for Case 5, a SOAP fault, a
//! transport failure or an answer that does not hold to the transaction. A
//! deployment broadcasts by asking each gateway it knows; the client keeps no
//! state between requests and caches no correlation (§3.55.4.2.3.1 leaves
//! caching to the deployment, and no `CorrelationTimeToLive` is sent).
//!
//! The patient identifier is a directly identifying value. It travels only
//! in the request body to the responding gateway, which is the transaction's
//! purpose; every type that carries it, and every identifier an answer
//! returns, redacts it in `Debug`, and no error carries a value, the request
//! or the gateway's free text. A SAML 2.0 XUA assertion
//! ([`security::XuaAssertion`]) the deployment supplies rides in a
//! WS-Security header; the crate signs nothing. Each exchange can record
//! its ITI-55 audit message through an [`audit::AuditRecorder`] the
//! deployment routes to its audit repository (§3.55.5.1.1).
//!
//! The asynchronous and deferred exchanges (§3.55.6.2, Appendix V.5) are not
//! offered: the initiating gateway claims neither option (ITI TF-1 §27.2),
//! and every request asks for an immediate response.
//!
//! # Examples
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use ihe_iti::xcpd::XcpdClient;
//! use ihe_iti::xcpd::discovery::Discovery;
//! use ihe_iti::xcpd::identifier::{Oid, PatientIdentifier};
//! use ihe_iti::user::OnBehalfOf;
//! use ihe_iti::xcpd::request::{DiscoveryQuery, RespondingGateway};
//! use secrecy::SecretString;
//! use url::Url;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let http = reqwest::Client::builder()
//!     .redirect(reqwest::redirect::Policy::none())
//!     .build()?;
//! let client = XcpdClient::new(http);
//! let gateway = RespondingGateway::new(
//!     Url::parse("https://xcpd.example.org/RespondingGateway")?,
//!     Oid::new("2.999.50.1")?,
//! )?;
//! let patient = PatientIdentifier::new(Oid::new("2.999.1")?, SecretString::from("9999"))?;
//! let query = DiscoveryQuery::new(Oid::new("2.999.40.1")?, patient);
//! let on_behalf = OnBehalfOf::System;
//! match client
//!     .discover(&gateway, &query, None, &on_behalf, Duration::from_secs(5))
//!     .await?
//! {
//!     Discovery::Matched(found) => {
//!         for record in found {
//!             let _community = record.community();
//!         }
//!     }
//!     Discovery::NoMatch => {}
//!     _ => {}
//! }
//! # Ok(())
//! # }
//! # fn main() {
//! #     let _pending = run();
//! # }
//! ```

pub mod audit;
pub mod discovery;
pub mod error;
pub mod identifier;
pub mod request;
mod response;
pub mod security;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use http::header::{ACCEPT, CONTENT_TYPE};

use crate::recording::{Late, Recorded, within};
use crate::user::OnBehalfOf;
use audit::{AuditEvent, AuditRecorder, EventOutcome, NetworkAccessPoint};
use discovery::Discovery;
use error::{Malformation, XcpdError};
use request::{DiscoveryQuery, Ids, RespondingGateway};
use security::XuaAssertion;

/// The SOAP 1.2 envelope namespace.
const SOAP: &str = "http://www.w3.org/2003/05/soap-envelope";

/// The WS-Addressing 1.0 namespace.
const WSA: &str = "http://www.w3.org/2005/08/addressing";

/// The WS-Security 1.0 extension namespace.
const WSSE: &str =
    "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd";

/// The HL7 v3 namespace.
const HL7: &str = "urn:hl7-org:v3";

/// The WS-Addressing action of the request (§3.55.6.1.1).
const ACTION: &str = "urn:hl7-org:v3:PRPA_IN201305UV02:CrossGatewayPatientDiscovery";

/// The code system of the attributes a gateway asks for (§3.55.4.2.2.6).
const REQUEST_SYSTEM: &str = "1.3.6.1.4.1.19376.1.2.27.1";

/// The code system of the problems a gateway reports (§3.55.4.2.2.7).
const ISSUE_SYSTEM: &str = "1.3.6.1.4.1.19376.1.2.27.3";

/// The SOAP 1.2 media type of the request, with the action parameter of the
/// SOAP 1.2 HTTP binding (RFC 3902).
const SOAP_REQUEST: &str = "application/soap+xml; charset=UTF-8; action=\"urn:hl7-org:v3:PRPA_IN201305UV02:CrossGatewayPatientDiscovery\"";

/// An XCPD Initiating Gateway, the client side of ITI-55.
///
/// `Debug` leaves out the HTTP client, whose configuration may hold a
/// credential.
#[derive(Clone)]
pub struct XcpdClient {
    http: reqwest::Client,
    audit: Option<Arc<dyn AuditRecorder>>,
}

impl XcpdClient {
    /// Creates a client sending through `http`.
    ///
    /// `http` carries the transport the caller chose: the mutual TLS of the
    /// ATNA Secure Node the actor is grouped with (ITI TF-1 Table 27.1.3-1),
    /// and the trust roots of the network. Build it with
    /// `redirect::Policy::none()`: the request body holds the patient
    /// identifier, and a client that follows a `307` sends it wherever the
    /// gateway points.
    #[must_use]
    pub fn new(http: reqwest::Client) -> Self {
        Self { http, audit: None }
    }

    /// This client, recording the audit message of every exchange through
    /// `recorder` (§3.55.5.1.1).
    #[must_use]
    pub fn audited(mut self, recorder: Arc<dyn AuditRecorder>) -> Self {
        self.audit = Some(recorder);
        self
    }

    /// Asks `gateway` whether its community knows the patient `query` names
    /// (§3.55.4.1), with `assertion` in the WS-Security header when given, on
    /// behalf of `on_behalf`, whom an audited client's message names as its
    /// Human Requestor (§3.55.5.1.1).
    ///
    /// `timeout` bounds the whole exchange, from connecting until the answer
    /// is read, and an audited client's record of it ([`crate::recording`]).
    ///
    /// # Errors
    /// An [`XcpdError`] for Case 5 of §3.55.4.2.3, a SOAP fault, an HTTP
    /// status with no ITI-55 answer, a timeout, a transport failure, and an
    /// answer that does not hold to ITI-55; when audited,
    /// [`XcpdError::Audit`] for an audit message the recorder refused, or
    /// did not accept within `timeout` for an exchange that succeeded.
    pub async fn discover(
        &self,
        gateway: &RespondingGateway,
        query: &DiscoveryQuery,
        assertion: Option<&XuaAssertion>,
        on_behalf: &OnBehalfOf,
        timeout: Duration,
    ) -> Result<Discovery, XcpdError> {
        let deadline = crate::recording::deadline(timeout);
        let ids = Ids::fresh();
        let written = request::envelope(query, gateway, &ids, assertion)?;
        let answer = self.exchange(gateway, written.body, &ids, timeout).await;
        if let Some(recorder) = &self.audit {
            let event = AuditEvent {
                date_time: jiff::Timestamp::now(),
                outcome: match &answer {
                    Ok(_) => EventOutcome::Success,
                    Err(error) if error.status().is_some() => EventOutcome::MinorFailure,
                    Err(_) => EventOutcome::SeriousFailure,
                },
                process_id: std::process::id(),
                destination: audit::destination(gateway.endpoint()),
                destination_access_point: NetworkAccessPoint::of(gateway.endpoint()),
                query: written.query,
                home_community: query.community().cloned(),
                on_behalf: on_behalf.clone(),
            };
            // NOTE: ITI TF-2 §3.55.5.1, ITI TF-1 Table 27.1.3-1: the actor shall record the
            // exchange, so an answer whose audit message was not accepted is not used.
            match within(deadline, recorder.record(event)).await {
                Recorded::Refused(error) => return Err(XcpdError::Audit(error)),
                Recorded::Late if answer.is_ok() => {
                    return Err(XcpdError::Audit(audit::AuditError(Box::new(Late))));
                }
                Recorded::Accepted | Recorded::Late => {}
            }
        }
        answer
    }

    /// Posts `body` to `gateway` and reads the answer.
    async fn exchange(
        &self,
        gateway: &RespondingGateway,
        body: Vec<u8>,
        ids: &Ids,
        timeout: Duration,
    ) -> Result<Discovery, XcpdError> {
        let response = self
            .http
            .post(gateway.endpoint().clone())
            .header(CONTENT_TYPE, SOAP_REQUEST)
            .header(ACCEPT, "application/soap+xml")
            .timeout(timeout)
            .body(body)
            .send()
            .await
            .map_err(transport)?;
        let status = response.status();
        let media = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let answer = body_of(response).await?;
        response::read(status, media.as_deref(), &answer, &ids.message_urn())
    }
}

impl fmt::Debug for XcpdClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("XcpdClient").finish_non_exhaustive()
    }
}

/// The answer's body, up to [`response::LIMIT`] bytes.
async fn body_of(mut response: reqwest::Response) -> Result<Vec<u8>, XcpdError> {
    let status = response.status();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport)? {
        if body.len().saturating_add(chunk.len()) > response::LIMIT {
            if status != http::StatusCode::OK {
                return Err(XcpdError::Rejected { status });
            }
            return Err(Malformation::TooLarge {
                limit: response::LIMIT,
            }
            .into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// A transport failure, with the request URL removed: the URL is the
/// gateway's, and may carry a credential in its userinfo.
fn transport(error: reqwest::Error) -> XcpdError {
    let error = error.without_url();
    if error.is_timeout() {
        XcpdError::Timeout
    } else {
        XcpdError::Transport(error)
    }
}
