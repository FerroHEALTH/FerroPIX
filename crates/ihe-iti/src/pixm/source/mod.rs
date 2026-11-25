// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Patient Identity Source side of ITI-104, Patient Identity Feed FHIR
//! (PIXm 3.1.0, ITI TF-2 §2:3.104, feature `pixm-source`).
//!
//! [`PixmSourceClient`] tells the Patient Identifier Cross-reference Manager
//! what happened to a patient identity in the Source's own domain:
//!
//! - [`feed`](PixmSourceClient::feed), Add or Revise Patient: the conditional
//!   update `PUT [base]/Patient?identifier=system|value` with the Patient as
//!   it now stands (§2:3.104.4.1);
//! - [`resolve_duplicate`](PixmSourceClient::resolve_duplicate), Resolve
//!   Duplicate Patient: the same update on the subsumed identifier, its
//!   Patient carrying a `replaced-by` link to the survivor (§2:3.104.4.2);
//! - [`remove`](PixmSourceClient::remove), Remove Patient: the conditional
//!   delete on the identifier (§2:3.104.4.3).
//!
//! Each answer is read as FHIR R4's conditional update and delete define it
//! (§2:3.104.4.1.4): [`Fed::Created`] for a `201`, [`Fed::Updated`] for a
//! `200`, [`Removed`] for a `200`, `204` or `202`, and an
//! [`error::SourceError`] for every other answer and for no answer, so a
//! refusal or an outage never reads as an applied message.
//!
//! The Patient and the identifier are identifying data. They travel only in
//! the request to the Manager: no type here has `Display`, every `Debug`
//! leaves values out, and no error carries a value, a URL, a body or the
//! Manager's free text.
//!
//! # Examples
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use ferrohealth_smart::client::outbound::OutboundClient;
//! use ferrohealth_smart::config::TlsBuilder;
//! use ferrohealth_smart::config::endpoint::EndpointSection;
//! use fhir_types::r4::identifier::Identifier;
//! use fhir_types::r4::patient::Patient;
//! use ihe_atna::user::OnBehalfOf;
//! use ihe_iti::pixm::identifier::SourceIdentifier;
//! use ihe_iti::pixm::source::{Fed, PixmSourceClient};
//! use secrecy::SecretString;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! # fn configured() -> (EndpointSection, &'static dyn TlsBuilder) {
//! #     unimplemented!("the product's configuration and its reader of TLS material")
//! # }
//! // The `[pixm.endpoint]` table, whose `url` is the Manager's FHIR base.
//! let (endpoint, tls) = configured();
//! let client = PixmSourceClient::new(OutboundClient::build(&endpoint, "pixm.endpoint", tls)?)?;
//! let identifier = SourceIdentifier::new(
//!     "urn:oid:1.3.6.1.4.1.21367.13.20.1000",
//!     SecretString::from("IHERED-994"),
//! )?;
//! let patient = Patient {
//!     identifier: vec![Identifier {
//!         system: Some("urn:oid:1.3.6.1.4.1.21367.13.20.1000".into()),
//!         value: Some("IHERED-994".into()),
//!         ..Identifier::default()
//!     }],
//!     ..Patient::default()
//! };
//! match client
//!     .feed(&identifier, &patient, &OnBehalfOf::System, Duration::from_secs(2))
//!     .await?
//! {
//!     Fed::Created(_receipt) | Fed::Updated(_receipt) => {}
//!     _ => {}
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

use std::fmt;
use std::future::Future;
#[cfg(feature = "balp")]
use std::sync::Arc;
use std::time::Duration;

use ferrohealth_smart::client::outbound::OutboundClient;
use fhir_types::r4::patient::Patient;
use http::Method;
use http::header::{ACCEPT, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::FHIR_JSON;
use super::error::InvalidInput;
use super::identifier::{PatientReference, SourceIdentifier};
use crate::outbound;
use error::SourceError;
use ihe_atna::redact::RedactedUrl;
use ihe_atna::user::OnBehalfOf;

/// How the Manager applied an Add or Revise Patient or a Resolve Duplicate
/// Patient message (FHIR R4 http.html §3.1.0.4).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Fed {
    /// `201 Created`: the identifier was new to the Manager.
    Created(Receipt),
    /// `200 OK`: the Manager knew the identifier and revised it.
    Updated(Receipt),
}

impl Fed {
    /// Returns what the Manager answered with.
    #[must_use]
    pub fn receipt(&self) -> &Receipt {
        match self {
            Self::Created(receipt) | Self::Updated(receipt) => receipt,
        }
    }
}

/// What the Manager answered beside the status of an applied update.
///
/// `Debug` redacts the location, since a logical id can be built from a
/// business identifier.
#[derive(Debug, Clone)]
pub struct Receipt {
    location: Option<PatientReference>,
}

impl Receipt {
    /// Returns the `Location` the Manager answered, the Patient it keeps for
    /// the identifier, when it named one (FHIR R4 http.html §3.1.0.4).
    #[must_use]
    pub fn location(&self) -> Option<&PatientReference> {
        self.location.as_ref()
    }
}

/// How the Manager applied a Remove Patient message (FHIR R4 http.html
/// §3.1.0.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Removed {
    /// `200 OK` or `204 No Content`: the identifier is removed, or the
    /// Manager held none.
    Deleted,
    /// `202 Accepted`: the Manager took the message and does not commit to
    /// its outcome.
    Accepted,
}

/// The message one ITI-104 exchange sends.
enum Message<'a> {
    /// A conditional update about `identifier`, with the Patient `body`.
    Update {
        identifier: &'a SourceIdentifier,
        body: SecretString,
    },
    /// A conditional delete of `identifier`.
    Delete { identifier: &'a SourceIdentifier },
}

impl Message<'_> {
    /// The identifier the message names in its condition.
    fn identifier(&self) -> &SourceIdentifier {
        match self {
            Self::Update { identifier, .. } | Self::Delete { identifier } => identifier,
        }
    }
}

/// A Patient Identity Source bound to one PIX Manager.
///
/// `Debug` shows the Manager's base with its userinfo replaced by `***` and
/// whether the client carries a credential, never the credential itself.
#[derive(Clone)]
pub struct PixmSourceClient {
    #[cfg(feature = "balp")]
    base: Url,
    patients: Url,
    client: OutboundClient,
    #[cfg(feature = "balp")]
    audit: Option<Arc<dyn ihe_atna::balp::AuditRecorder>>,
}

impl PixmSourceClient {
    /// Creates a client for the PIX Manager whose FHIR base URL is
    /// `client`'s URL, reached through `client`.
    ///
    /// `client` carries the transport its endpoint table names: TLS, client
    /// certificates, the credential every request carries (ITI TF-2 Appendix
    /// Z.8, IUA ITI-72 with the `ITI-104` scope, §2:3.104.5.2) and the retry
    /// table. It follows no redirect, so the Patient goes nowhere the base
    /// does not name.
    ///
    /// # Errors
    /// [`InvalidInput::Base`] when the URL is not an `http` or `https` URL
    /// without a query or a fragment.
    pub fn new(client: OutboundClient) -> Result<Self, InvalidInput> {
        let patients = ihe_atna::fhir_base::under_base(client.url().clone(), request::PATIENT)
            .ok_or(InvalidInput::Base)?;
        Ok(Self {
            #[cfg(feature = "balp")]
            base: patients.join(".").map_err(|_relative| InvalidInput::Base)?,
            patients,
            client,
            #[cfg(feature = "balp")]
            audit: None,
        })
    }

    /// This client, recording the audit record of every exchange through
    /// `recorder` (§2:3.104.5.1.1, feature `balp`).
    #[cfg(feature = "balp")]
    #[must_use]
    pub fn audited(mut self, recorder: Arc<dyn ihe_atna::balp::AuditRecorder>) -> Self {
        self.audit = Some(recorder);
        self
    }

    /// Returns the `[base]/Patient` URL whose conditional interactions the
    /// client sends.
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.patients
    }

    /// Add or Revise Patient (§2:3.104.4.1): tells the Manager that the
    /// patient `identifier` names in the Source's domain now stands as
    /// `patient`, on behalf of `on_behalf`.
    ///
    /// `timeout` bounds the whole exchange and an audited client's record of
    /// it. An audited client records the exchange before it returns,
    /// whatever its outcome; an exchange whose record is not accepted fails
    /// ([`ihe_atna::balp::recording`]).
    ///
    /// # Errors
    /// [`SourceError::Invalid`] before anything is sent when `patient` does
    /// not hold to the PDQm Patient profile, does not carry `identifier`, or
    /// carries a `replaced-by` link; [`SourceError::Unwritable`] when it
    /// cannot be written; [`SourceError::NotSelective`] or
    /// [`SourceError::Rejected`] for an answer that does not apply it; a
    /// [`SourceError`] for no answer; and, when audited,
    /// [`SourceError::Audit`] for a record not accepted.
    pub async fn feed(
        &self,
        identifier: &SourceIdentifier,
        patient: &Patient,
        on_behalf: &OnBehalfOf,
        timeout: Duration,
    ) -> Result<Fed, SourceError> {
        let body = request::revision(identifier, patient)?;
        self.update(Message::Update { identifier, body }, on_behalf, timeout)
            .await
    }

    /// Resolve Duplicate Patient (§2:3.104.4.2): tells the Manager that the
    /// Source merged the patient `subsumed` names into the one `survivor`
    /// names, in the same domain, sending `patient`, the subsumed patient,
    /// with a `replaced-by` link to `survivor` (§2:3.104.4.2.2).
    ///
    /// `timeout` and the audit record are as for [`feed`](Self::feed); the
    /// record names the subsumed identifier, the one the request names.
    ///
    /// # Errors
    /// [`SourceError::Invalid`] before anything is sent when the survivor is
    /// in another domain or is the subsumed identifier, or `patient` does
    /// not hold to the profile, does not carry `subsumed` or carries a
    /// `replaced-by` link of its own; otherwise as for
    /// [`feed`](Self::feed).
    pub async fn resolve_duplicate(
        &self,
        subsumed: &SourceIdentifier,
        survivor: &SourceIdentifier,
        patient: &Patient,
        on_behalf: &OnBehalfOf,
        timeout: Duration,
    ) -> Result<Fed, SourceError> {
        let body = request::merge(subsumed, survivor, patient)?;
        self.update(
            Message::Update {
                identifier: subsumed,
                body,
            },
            on_behalf,
            timeout,
        )
        .await
    }

    /// Remove Patient (§2:3.104.4.3): tells a Manager that supports the
    /// Remove Patient Option that the Source removed the patient
    /// `identifier` names, on behalf of `on_behalf`.
    ///
    /// `timeout` and the audit record are as for [`feed`](Self::feed).
    ///
    /// # Errors
    /// [`SourceError::NotSelective`] or [`SourceError::Rejected`] for an
    /// answer that does not apply it, a [`SourceError`] for no answer, and,
    /// when audited, [`SourceError::Audit`] for a record not accepted.
    pub async fn remove(
        &self,
        identifier: &SourceIdentifier,
        on_behalf: &OnBehalfOf,
        timeout: Duration,
    ) -> Result<Removed, SourceError> {
        self.exchange(
            Message::Delete { identifier },
            on_behalf,
            timeout,
            response::delete,
            |_removed| None,
        )
        .await
    }

    /// Sends the conditional update `message` and reads its answer.
    async fn update(
        &self,
        message: Message<'_>,
        on_behalf: &OnBehalfOf,
        timeout: Duration,
    ) -> Result<Fed, SourceError> {
        self.exchange(message, on_behalf, timeout, response::update, |fed| {
            fed.receipt().location()
        })
        .await
    }

    /// Sends `message`, reads its answer with `read` and, when audited,
    /// records the exchange with the Patient `located` finds in the outcome.
    async fn exchange<T, Read, Answer>(
        &self,
        message: Message<'_>,
        #[cfg_attr(
            not(feature = "balp"),
            expect(
                unused_variables,
                reason = "only an audited client names whom it acted for"
            )
        )]
        on_behalf: &OnBehalfOf,
        timeout: Duration,
        read: Read,
        #[cfg_attr(
            not(feature = "balp"),
            expect(unused_variables, reason = "only an audit record names the Patient")
        )]
        located: fn(&T) -> Option<&PatientReference>,
    ) -> Result<T, SourceError>
    where
        Read: FnOnce(reqwest::Response) -> Answer,
        Answer: Future<Output = Result<T, SourceError>>,
    {
        #[cfg(feature = "balp")]
        let deadline = ihe_atna::balp::recording::deadline(timeout);
        let result = match self.send(&message, timeout).await {
            Ok(answer) => read(answer).await,
            Err(error) => Err(error),
        };
        #[cfg(feature = "balp")]
        if let Some(recorder) = &self.audit {
            let action = match message {
                Message::Update { .. } => super::audit::FeedAction::Update,
                Message::Delete { .. } => super::audit::FeedAction::Delete,
            };
            let exchange = super::audit::source_feed(
                &self.base,
                action,
                message.identifier(),
                result.as_ref().ok().and_then(located),
                on_behalf,
                result.as_ref().map(|_applied| ()),
            );
            // NOTE: PIXm §2:3.104.5.1.1 makes the Source record each feed, so an
            // outcome whose record was not accepted is not used.
            return crate::audited::settled(
                deadline,
                recorder.record(exchange),
                result,
                SourceError::Audit,
            )
            .await;
        }
        result
    }

    /// Sends `message` within `timeout`.
    async fn send(
        &self,
        message: &Message<'_>,
        timeout: Duration,
    ) -> Result<reqwest::Response, SourceError> {
        let url = request::conditional(&self.patients, message.identifier());
        // NOTE: PIXm §2:3.104.4.1.2, a Source "shall be able to send" either JSON or
        // XML, and the Manager supports both, so the Source sends JSON.
        match message {
            Message::Update { body, .. } => {
                outbound::send::<SourceError, _>(
                    &self.client,
                    &Method::PUT,
                    &url,
                    timeout,
                    |request| {
                        request
                            .header(ACCEPT, FHIR_JSON)
                            .header(CONTENT_TYPE, FHIR_JSON)
                            .body(body.expose_secret().as_bytes().to_vec())
                    },
                )
                .await
            }
            Message::Delete { .. } => {
                outbound::send::<SourceError, _>(
                    &self.client,
                    &Method::DELETE,
                    &url,
                    timeout,
                    |request| request.header(ACCEPT, FHIR_JSON),
                )
                .await
            }
        }
    }
}

impl fmt::Debug for PixmSourceClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PixmSourceClient")
            .field("endpoint", &RedactedUrl(self.patients.as_str()))
            .field("authorization", &self.client.authorization().is_some())
            .finish_non_exhaustive()
    }
}
