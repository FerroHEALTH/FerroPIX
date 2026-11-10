// SPDX-FileCopyrightText: Cadasto B.V.
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
//! [`PixmClient::capabilities`] reads the PIX Manager's `CapabilityStatement`
//! (`GET [base]/metadata`), which a consumer asks at boot to learn that the
//! configured base is a FHIR server that answers, through the same transport
//! and credential as ITI-83.
//!
//! [`PixmClient::cross_reference`] makes the HTTP request and reads the answer
//! into an [`identifier::CrossReference`] or an [`error::PixmError`]. The
//! request is the `GET` ITI-83 prescribes, or, for a client
//! [invoked](PixmClient::invoked_by) with [`Invocation::Post`], the same input
//! parameters posted in a `Parameters` body, which keeps the source identifier
//! out of the request URL. The two
//! answers the profile defines for an unknown patient (§2:3.83.4.2.2.2 and
//! §2:3.83.4.2.2.5) are `CrossReference::SourceNotFound`; every other failure,
//! including a `404` that carries no `not-found` issue, is an error, so an
//! outage or a misrouted request never reads as a patient with no identifiers.
//!
//! The source identifier is a directly identifying value. It travels only in
//! the request to the PIX Manager, which is the transaction's purpose: the
//! types that carry it and every identifier the Manager returns redact it in
//! `Debug`, none of them has `Display`, and no error this module returns
//! carries a value, a request URL or body, or the Manager's free text.
//!
//! # Examples
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use ferrohealth_smart::client::outbound::OutboundClient;
//! use ferrohealth_smart::config::TlsBuilder;
//! use ferrohealth_smart::config::endpoint::EndpointSection;
//! use ihe_iti::pixm::PixmClient;
//! use ihe_iti::pixm::identifier::{CrossReference, SourceIdentifier, TargetSystem};
//! use ihe_atna::user::OnBehalfOf;
//! use secrecy::{ExposeSecret, SecretString};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! # fn configured() -> (EndpointSection, &'static dyn TlsBuilder) {
//! #     unimplemented!("the product's configuration and its reader of TLS material")
//! # }
//! // The `[pixm.endpoint]` table, whose `url` is the Manager's FHIR base.
//! let (endpoint, tls) = configured();
//! let client = PixmClient::new(OutboundClient::build(&endpoint, "pixm.endpoint", tls)?)?;
//! let source = SourceIdentifier::new(
//!     "urn:oid:1.3.6.1.4.1.21367.13.20.1000",
//!     SecretString::from("IHERED-994"),
//! )?;
//! let targets = [TargetSystem::new("urn:oid:1.3.6.1.4.1.21367.13.20.3000")?];
//! let on_behalf = OnBehalfOf::System;
//! match client
//!     .cross_reference(&source, &targets, &on_behalf, Duration::from_secs(2))
//!     .await?
//! {
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

#[cfg(feature = "balp")]
pub mod audit;
pub mod capabilities;
pub mod error;
pub mod identifier;
mod request;
mod response;
#[cfg(feature = "pixm-server")]
pub mod server;

use std::fmt;
#[cfg(feature = "balp")]
use std::sync::Arc;
use std::time::Duration;

use ferrohealth_smart::client::outbound::OutboundClient;
use http::Method;
use http::header::{ACCEPT, CONTENT_TYPE};
use secrecy::ExposeSecret;
use url::Url;

use crate::exchange;
use crate::outbound;
use capabilities::CapabilitiesError;
use error::{InvalidInput, PixmError};
use fhir_types::r4::capability_statement::CapabilityStatement;
use identifier::{CrossReference, SourceIdentifier, TargetSystem};
use ihe_atna::redact::RedactedUrl;
use ihe_atna::user::OnBehalfOf;
use request::Request;

/// The media type ITI-83 asks for and reads (ITI TF-2 Appendix Z.6).
const FHIR_JSON: &str = "application/fhir+json";

/// How the client invokes the `$ihe-pix` operation.
///
/// FHIR R4 invokes an operation "generally" by an HTTP `POST` of a
/// `Parameters` resource to the operation's endpoint, and admits a `GET` with
/// the parameters in the URL for an operation that does not affect state and
/// takes primitive parameters only, which "Servers SHALL support" (FHIR R4
/// Operations, §3.2.0.1, <http://hl7.org/fhir/R4/operations.html#executing>).
/// ITI-83 prescribes the `GET` (§2:3.83.4.1.2) and says nothing of a `POST`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Invocation {
    /// `GET [base]/Patient/$ihe-pix?sourceIdentifier=…{&targetSystem=…}`, the
    /// form ITI-83 prescribes and FHIR R4 requires a server to support. The
    /// source identifier is in the request URL.
    #[default]
    Get,
    /// `POST [base]/Patient/$ihe-pix` with the same input parameters in a
    /// `Parameters` body, as the Query Parameters In profile and the IG's
    /// request example give them. The request URL holds no input parameter,
    /// so the source identifier stays out of the access logs that record
    /// URLs; ask a Manager this way only when it says it accepts it.
    Post,
}

/// A Patient Identifier Cross-reference Consumer bound to one PIX Manager.
///
/// `Debug` shows the endpoint with its userinfo replaced by `***`, the
/// invocation and whether the client carries a credential, never the
/// credential itself.
#[derive(Clone)]
pub struct PixmClient {
    endpoint: Url,
    metadata: Url,
    invocation: Invocation,
    client: OutboundClient,
    #[cfg(feature = "balp")]
    audit: Option<Arc<dyn ihe_atna::balp::AuditRecorder>>,
}

impl PixmClient {
    /// Creates a client for the PIX Manager whose FHIR base URL is
    /// `client`'s URL, reached through `client`.
    ///
    /// `client` carries the transport its endpoint table names: TLS, client
    /// certificates, the credential every request carries (ITI TF-2 Appendix
    /// Z.8) and the retry table. It follows no redirect, so the source
    /// identifier goes nowhere the base does not name, and a `3xx` is an
    /// error like any other unexpected status.
    ///
    /// The client asks with [`Invocation::Get`] until
    /// [`invoked_by`](Self::invoked_by) says otherwise.
    ///
    /// # Errors
    /// [`InvalidInput::Base`] when the URL is not an `http` or `https` URL
    /// without a query or a fragment.
    pub fn new(client: OutboundClient) -> Result<Self, InvalidInput> {
        let base = client.url().clone();
        let metadata = ihe_atna::fhir_base::under_base(base.clone(), capabilities::INTERACTION)
            .ok_or(InvalidInput::Base)?;
        Ok(Self {
            endpoint: request::endpoint(base)?,
            metadata,
            invocation: Invocation::default(),
            client,
            #[cfg(feature = "balp")]
            audit: None,
        })
    }

    /// This client, asking the Manager with `invocation`.
    #[must_use]
    pub fn invoked_by(mut self, invocation: Invocation) -> Self {
        self.invocation = invocation;
        self
    }

    /// Returns how the client asks the Manager.
    #[must_use]
    pub fn invocation(&self) -> Invocation {
        self.invocation
    }

    /// This client, recording the audit record of every exchange through
    /// `recorder` (§2:3.83.5.1.1, feature `balp`).
    #[cfg(feature = "balp")]
    #[must_use]
    pub fn audited(mut self, recorder: Arc<dyn ihe_atna::balp::AuditRecorder>) -> Self {
        self.audit = Some(recorder);
        self
    }

    /// Returns the `[base]/Patient/$ihe-pix` URL the client asks, which a
    /// `GET` extends with its query (§2:3.83.4.1.2).
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Reads the PIX Manager's `CapabilityStatement` with `GET [base]/metadata`
    /// (FHIR R4 http.html §capabilities, <http://hl7.org/fhir/R4/http.html#capabilities>),
    /// through this client's transport and credential, within `timeout`.
    ///
    /// The request names no patient, so it carries no audit record of its own.
    ///
    /// # Errors
    /// [`CapabilitiesError::Refused`] with the status for an answer other than
    /// `200`; [`CapabilitiesError::NotAFhirServer`] for a `200` whose body is no
    /// FHIR R4 `CapabilityStatement`; [`CapabilitiesError::Timeout`],
    /// [`CapabilitiesError::Transport`] or
    /// [`CapabilitiesError::Unauthenticated`] when no answer was had.
    pub async fn capabilities(
        &self,
        timeout: Duration,
    ) -> Result<CapabilityStatement, CapabilitiesError> {
        let response = outbound::send::<CapabilitiesError, _>(
            &self.client,
            &Method::GET,
            &self.metadata,
            timeout,
            |request| request.header(ACCEPT, FHIR_JSON),
        )
        .await?;
        let answer = exchange::read(response, capabilities::LIMIT, capabilities::too_large).await?;
        capabilities::read(answer.status, answer.media.as_deref(), &answer.body)
    }

    /// Asks the PIX Manager for the identifiers the `targets` domains hold for
    /// the patient `source` names, or every domain's when `targets` is empty
    /// (§2:3.83.4.1.2.2), on behalf of `on_behalf`.
    ///
    /// `timeout` bounds the whole exchange, from connecting until the answer is
    /// read, and an audited client's record of it.
    ///
    /// An audited client records the exchange before it returns, whatever
    /// its outcome, naming the user `on_behalf` names, from their OAuth token
    /// (§2:3.83.5.2.1); an exchange whose record the recorder does not accept
    /// fails, and its answer is not used (§2:3.83.5.1.1). So does an
    /// exchange that succeeded and whose record is not accepted within
    /// `timeout` ([`ihe_atna::balp::recording`]).
    ///
    /// # Errors
    /// [`PixmError::Unwritable`] before anything is sent when the
    /// `Parameters` body of a `POST` cannot be written; a [`PixmError`] for
    /// every answer that is neither a cross-reference nor one of the
    /// profile's two not-found answers, for a failure to get an answer at
    /// all, and, when audited, [`PixmError::Audit`] for a record the recorder
    /// refused or did not accept in time.
    pub async fn cross_reference(
        &self,
        source: &SourceIdentifier,
        targets: &[TargetSystem],
        #[cfg_attr(
            not(feature = "balp"),
            expect(
                unused_variables,
                reason = "only an audited client names whom it acted for"
            )
        )]
        on_behalf: &OnBehalfOf,
        timeout: Duration,
    ) -> Result<CrossReference, PixmError> {
        #[cfg(feature = "balp")]
        let deadline = ihe_atna::balp::recording::deadline(timeout);
        let request = request::request(&self.endpoint, self.invocation, source, targets)
            .map_err(PixmError::Unwritable)?;
        let result = self.ask(&request, source, targets, timeout).await;
        #[cfg(feature = "balp")]
        if let Some(recorder) = &self.audit {
            let exchange = audit::exchange(&self.endpoint, (&request, source), on_behalf, &result);
            // NOTE: PIXm §2:3.83.5.1.1 makes the audit record part of the exchange,
            // so an answer whose record was not accepted is not used.
            return crate::audited::settled(
                deadline,
                recorder.record(exchange),
                result,
                PixmError::Audit,
            )
            .await;
        }
        result
    }

    async fn ask(
        &self,
        request: &Request,
        source: &SourceIdentifier,
        targets: &[TargetSystem],
        timeout: Duration,
    ) -> Result<CrossReference, PixmError> {
        let response = match request {
            Request::Get(url) => {
                outbound::send::<PixmError, _>(
                    &self.client,
                    &Method::GET,
                    url,
                    timeout,
                    |request| request.header(ACCEPT, FHIR_JSON),
                )
                .await?
            }
            Request::Post(body) => {
                outbound::send::<PixmError, _>(
                    &self.client,
                    &Method::POST,
                    &self.endpoint,
                    timeout,
                    |request| {
                        request
                            .header(ACCEPT, FHIR_JSON)
                            .header(CONTENT_TYPE, FHIR_JSON)
                            .body(body.expose_secret().as_bytes().to_vec())
                    },
                )
                .await?
            }
        };
        let answer = exchange::read(response, response::LIMIT, response::too_large).await?;
        response::read(
            answer.status,
            answer.media.as_deref(),
            &answer.body,
            source,
            targets,
        )
    }
}

impl fmt::Debug for PixmClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PixmClient")
            .field("endpoint", &RedactedUrl(self.endpoint.as_str()))
            .field("invocation", &self.invocation)
            .field("authorization", &self.client.authorization().is_some())
            .finish_non_exhaustive()
    }
}
