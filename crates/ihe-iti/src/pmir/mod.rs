// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! PMIR, Patient Master Identity Registry (feature `pmir`).
//!
//! The Patient Identity Subscriber and Patient Identity Consumer sides of
//! PMIR 1.6.0 (ITI TF-1 §1:49):
//!
//! - **ITI-94, Subscribe to Patient Updates** (§2:3.94): [`PmirSubscriber`]
//!   creates a `Subscription` at the Patient Identity Registry
//!   ([`subscription::SubscriptionRequest`]), reads its status and deletes
//!   it.
//! - **ITI-93, Mobile Patient Identity Feed** (§2:3.93): [`feed::Feed::read`]
//!   reads a message the Registry sends to the subscription's channel into
//!   typed creates, updates, deletes and merges, and refuses one that departs
//!   from the PMIR profiles; [`feed::Feed::acknowledgement`] writes the
//!   response.
//!
//! An ITI-93 message carries Patient Master Identities: business identifiers
//! and demographics. The feed keeps the resource ids and the identifiers
//! only, each value in a `SecretString`, and drops every demographic as it
//! reads. No type here has `Display`, `Debug` shows no value, and no error
//! carries a value, a request URL or text the Registry wrote.
//!
//! How the Registry authenticates to the Consumer's channel is left to the
//! two parties: the subscription carries no credential for the feed
//! (§2:3.94.5), and ITI-93 admits mutual TLS, IUA or "other solutions … as
//! appropriate agreement between client and server" (§2:3.93.5). A caller
//! authenticates each message before it reads it.
//!
//! # Examples
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use ferrohealth_smart::client::outbound::OutboundClient;
//! use ferrohealth_smart::config::TlsBuilder;
//! use ferrohealth_smart::config::endpoint::EndpointSection;
//! use ihe_iti::pmir::PmirSubscriber;
//! use ihe_iti::pmir::subscription::{Criteria, SubscriptionRequest};
//! use url::Url;
//!
//! # fn configured() -> (EndpointSection, &'static dyn TlsBuilder) {
//! #     unimplemented!("the product's configuration and its reader of TLS material")
//! # }
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! // The `[pmir.endpoint]` table, whose `url` is the Registry's FHIR base.
//! let (endpoint, tls) = configured();
//! let subscriber = PmirSubscriber::new(OutboundClient::build(&endpoint, "pmir.endpoint", tls)?)?;
//! let request = SubscriptionRequest::new(
//!     Criteria::identifier_system("urn:oid:2.999.1")?,
//!     Url::parse("https://gateway.example.org/pmir/feed")?,
//! )?;
//! let subscribed = subscriber.subscribe(&request, Duration::from_secs(5)).await?;
//! let _status = subscriber.status(&subscribed, Duration::from_secs(5)).await?;
//! # Ok(())
//! # }
//! # fn main() {
//! #     let _pending = run();
//! # }
//! ```

#[cfg(feature = "balp")]
pub mod audit;
pub mod error;
pub mod feed;
mod message;
#[cfg(feature = "pmir-server")]
pub mod server;
pub mod subscription;

use std::fmt;
#[cfg(feature = "balp")]
use std::sync::Arc;
use std::time::Duration;

use ferrohealth_smart::client::outbound::OutboundClient;
use http::header::{ACCEPT, CONTENT_TYPE, LOCATION};
use http::{Method, StatusCode};
use url::Url;

use crate::exchange::{self, Answer};
use crate::outbound;
use crate::outcome;
use error::{InvalidInput, SubscribeError, SubscriptionMalformation};
use ihe_atna::redact::RedactedUrl;
use subscription::{FHIR_JSON, Subscribed, SubscriptionRequest, SubscriptionStatus};

/// The longest answer the subscriber reads: a `Subscription` or an
/// `OperationOutcome`, a few kilobytes at most.
const LIMIT: usize = 1 << 20;

/// A Patient Identity Subscriber bound to one Patient Identity Registry.
///
/// `Debug` shows the endpoint with its userinfo replaced by `***` and
/// whether the subscriber carries a credential, never the credential
/// itself.
#[derive(Clone)]
pub struct PmirSubscriber {
    endpoint: Url,
    client: OutboundClient,
    #[cfg(feature = "balp")]
    audit: Option<Arc<dyn ihe_atna::balp::AuditRecorder>>,
}

impl PmirSubscriber {
    /// Creates a subscriber for the Registry whose FHIR base URL is
    /// `client`'s URL, reached through `client`.
    ///
    /// `client` carries the transport its endpoint table names: TLS, client
    /// certificates and the credential every request carries, by which the
    /// Registry authorizes the subscriber (§2:3.94.5), and the retry table.
    /// It follows no redirect, so the credential goes nowhere the base does
    /// not name.
    ///
    /// # Errors
    /// [`InvalidInput::Base`] when the URL is not an `http` or `https` URL
    /// without a query or a fragment.
    pub fn new(client: OutboundClient) -> Result<Self, InvalidInput> {
        let endpoint = ihe_atna::fhir_base::under_base(client.url().clone(), "Subscription")
            .ok_or(InvalidInput::Base)?;
        Ok(Self {
            endpoint,
            client,
            #[cfg(feature = "balp")]
            audit: None,
        })
    }

    /// Sends a `method` request to `url`, shaped by `prepare` and asking for
    /// FHIR JSON, through the outbound client within `timeout`.
    async fn send(
        &self,
        method: &Method,
        url: &Url,
        timeout: Duration,
        prepare: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, SubscribeError> {
        outbound::send(&self.client, method, url, timeout, |request| {
            prepare(request).header(ACCEPT, FHIR_JSON)
        })
        .await
    }

    /// Sends a `method` request with no body to `url` as
    /// [`PmirSubscriber::send`] does, and reads its answer whole.
    async fn exchange(
        &self,
        method: &Method,
        url: &Url,
        timeout: Duration,
    ) -> Result<Answer, SubscribeError> {
        let response = self.send(method, url, timeout, |request| request).await?;
        exchange::read(response, LIMIT, too_large).await
    }

    /// This subscriber, recording the audit record of every exchange with
    /// the Registry through `recorder` (§2:3.94.5.1, feature `balp`).
    ///
    /// An audited subscriber records each exchange before it returns,
    /// whatever its outcome; one whose record the recorder does not accept
    /// fails with [`SubscribeError::Audit`], and its answer is not used.
    #[cfg(feature = "balp")]
    #[must_use]
    pub fn audited(mut self, recorder: Arc<dyn ihe_atna::balp::AuditRecorder>) -> Self {
        self.audit = Some(recorder);
        self
    }

    /// Records the exchange `exchange` describes, when the subscriber is
    /// audited, and returns `result` unless the record was refused.
    #[cfg(feature = "balp")]
    async fn audit<T>(
        &self,
        exchange: impl FnOnce(&Result<T, SubscribeError>) -> ihe_atna::balp::Exchange,
        result: Result<T, SubscribeError>,
    ) -> Result<T, SubscribeError> {
        if let Some(recorder) = &self.audit {
            // NOTE: PMIR §2:3.94.5.1 makes the audit record part of the exchange, so
            // an answer whose record was refused is not used.
            recorder
                .record(exchange(&result))
                .await
                .map_err(SubscribeError::Audit)?;
        }
        result
    }

    /// Returns the `[base]/Subscription` URL subscriptions are created at.
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Searches the Registry for the subscriptions `request` describes: an
    /// HTTP `GET [base]/Subscription?url=<endpoint>`, the R4 `Subscription`
    /// search parameter on the channel endpoint (FHIR R4 subscription.html,
    /// Search Parameters).
    ///
    /// A subscriber finds with it a subscription it created and never learned
    /// the location of, so it adopts that one in place of creating another.
    /// Only the subscriptions whose criteria, `message` channel and endpoint
    /// are the request's are listed.
    ///
    /// # Errors
    /// [`SubscribeError::Rejected`] for any status but `200`, `400` and `404`,
    /// the last two being [`subscription::Search::Unsupported`];
    /// [`SubscribeError::Malformed`] for an answer that is no searchset of
    /// `Subscription`s with ids and statuses; and [`SubscribeError::Timeout`]
    /// or [`SubscribeError::Transport`] when no answer arrives.
    pub async fn find(
        &self,
        request: &SubscriptionRequest,
        timeout: Duration,
    ) -> Result<subscription::Search, SubscribeError> {
        let mut url = self.endpoint.clone();
        url.query_pairs_mut()
            .append_pair("url", request.endpoint().as_str());
        let result = self.find_at(&url, request, timeout).await;
        #[cfg(feature = "balp")]
        let result = self
            .audit(|result| audit::search(&self.endpoint, &url, result), result)
            .await;
        result
    }

    async fn find_at(
        &self,
        url: &Url,
        request: &SubscriptionRequest,
        timeout: Duration,
    ) -> Result<subscription::Search, SubscribeError> {
        let answer = self.exchange(&Method::GET, url, timeout).await?;
        if matches!(
            answer.status,
            StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
        ) {
            return Ok(subscription::Search::Unsupported);
        }
        if answer.status != StatusCode::OK {
            return Err(rejected(&answer));
        }
        Ok(subscription::Search::Found(subscription::listed(
            answer.media.as_deref(),
            &answer.body,
            request,
            &self.endpoint,
        )?))
    }

    /// Creates the subscription `request` describes: an HTTP `POST` of the
    /// `Subscription` resource, answered `201` with its `Location`
    /// (§2:3.94.4.1.2, §2:3.94.4.2.2).
    ///
    /// `timeout` bounds the whole exchange.
    ///
    /// # Errors
    /// [`SubscribeError::Rejected`] for any status but `201`,
    /// [`SubscribeError::Malformed`] for a `201` whose `Location` is missing
    /// or names no `Subscription` under the base, and
    /// [`SubscribeError::Timeout`] or [`SubscribeError::Transport`] when no
    /// answer arrives.
    pub async fn subscribe(
        &self,
        request: &SubscriptionRequest,
        timeout: Duration,
    ) -> Result<Subscribed, SubscribeError> {
        let result = self.create(request, timeout).await;
        #[cfg(feature = "balp")]
        let result = self
            .audit(
                |result| {
                    let reference = result.as_ref().ok().and_then(|subscribed| {
                        audit::subscription_reference(subscribed.location())
                    });
                    audit::subscription(
                        audit::subscription_create(),
                        &self.endpoint,
                        reference,
                        Some(request.criteria().text()),
                        result,
                    )
                },
                result,
            )
            .await;
        result
    }

    async fn create(
        &self,
        request: &SubscriptionRequest,
        timeout: Duration,
    ) -> Result<Subscribed, SubscribeError> {
        let body = serde_json::to_vec(&request.resource()).map_err(SubscribeError::Unwritable)?;
        let response = self
            .send(&Method::POST, &self.endpoint, timeout, |request| {
                request.header(CONTENT_TYPE, FHIR_JSON).body(body.clone())
            })
            .await?;
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let answer = exchange::read(response, LIMIT, too_large).await?;
        if answer.status != StatusCode::CREATED {
            return Err(rejected(&answer));
        }
        let location = location.ok_or(SubscriptionMalformation::NoLocation)?;
        Ok(Subscribed::at(&self.endpoint, &location)?)
    }

    /// Reads the current status of `subscribed`: an HTTP `GET` of its
    /// location (§2:3.94.4.3).
    ///
    /// # Errors
    /// [`SubscribeError::Rejected`] for any status but `200`, among them the
    /// `404` or `410` of a subscription the Registry no longer holds,
    /// [`SubscribeError::Malformed`] for an answer that is no `Subscription`
    /// with a status of the value set, and [`SubscribeError::Timeout`] or
    /// [`SubscribeError::Transport`] when no answer arrives.
    pub async fn status(
        &self,
        subscribed: &Subscribed,
        timeout: Duration,
    ) -> Result<SubscriptionStatus, SubscribeError> {
        let result = self.read(subscribed, timeout).await;
        #[cfg(feature = "balp")]
        let result = self
            .audit(
                |result| {
                    audit::subscription(
                        audit::subscription_read(),
                        &self.endpoint,
                        audit::subscription_reference(subscribed.location()),
                        None,
                        result,
                    )
                },
                result,
            )
            .await;
        result
    }

    async fn read(
        &self,
        subscribed: &Subscribed,
        timeout: Duration,
    ) -> Result<SubscriptionStatus, SubscribeError> {
        let answer = self
            .exchange(&Method::GET, subscribed.location(), timeout)
            .await?;
        if answer.status != StatusCode::OK {
            return Err(rejected(&answer));
        }
        Ok(subscription::status(answer.media.as_deref(), &answer.body)?)
    }

    /// Deletes `subscribed`, so the Registry stops sending the feed: an HTTP
    /// `DELETE` of its location (§2:3.94.4.5).
    ///
    /// A `200`, `202` or `204` is a deletion, and so is the `404` or `410` of
    /// a subscription the Registry no longer holds (FHIR R4 http.html,
    /// delete).
    ///
    /// # Errors
    /// [`SubscribeError::Rejected`] for any other status, and
    /// [`SubscribeError::Timeout`] or [`SubscribeError::Transport`] when no
    /// answer arrives.
    pub async fn unsubscribe(
        &self,
        subscribed: &Subscribed,
        timeout: Duration,
    ) -> Result<(), SubscribeError> {
        let result = self.delete(subscribed, timeout).await;
        #[cfg(feature = "balp")]
        let result = self
            .audit(
                |result| {
                    audit::subscription(
                        audit::subscription_delete(),
                        &self.endpoint,
                        audit::subscription_reference(subscribed.location()),
                        None,
                        result,
                    )
                },
                result,
            )
            .await;
        result
    }

    async fn delete(
        &self,
        subscribed: &Subscribed,
        timeout: Duration,
    ) -> Result<(), SubscribeError> {
        let answer = self
            .exchange(&Method::DELETE, subscribed.location(), timeout)
            .await?;
        if matches!(
            answer.status,
            StatusCode::OK
                | StatusCode::ACCEPTED
                | StatusCode::NO_CONTENT
                | StatusCode::NOT_FOUND
                | StatusCode::GONE
        ) {
            return Ok(());
        }
        Err(rejected(&answer))
    }
}

impl fmt::Debug for PmirSubscriber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PmirSubscriber")
            .field("endpoint", &RedactedUrl(self.endpoint.as_str()))
            .field("authorization", &self.client.authorization().is_some())
            .finish_non_exhaustive()
    }
}

/// The refusal of `answer`, with the issue types of the `OperationOutcome`
/// it carries, if any.
fn rejected(answer: &Answer) -> SubscribeError {
    SubscribeError::Rejected {
        status: answer.status,
        issues: outcome::issues(answer.media.as_deref(), &answer.body),
    }
}

/// The refusal of an answer longer than [`LIMIT`], whatever its status.
fn too_large(_status: StatusCode) -> SubscribeError {
    SubscriptionMalformation::TooLarge { limit: LIMIT }.into()
}
