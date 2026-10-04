// SPDX-FileCopyrightText: Vernum Projecten B.V.
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
//! use ihe_iti::pmir::PmirSubscriber;
//! use ihe_iti::pmir::subscription::{Criteria, SubscriptionRequest};
//! use url::Url;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let http = reqwest::Client::builder()
//!     .redirect(reqwest::redirect::Policy::none())
//!     .build()?;
//! let subscriber = PmirSubscriber::new(Url::parse("https://pmir.example.org/fhir/")?, http)?;
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
pub mod subscription;

use std::fmt;
#[cfg(feature = "balp")]
use std::sync::Arc;
use std::time::Duration;

use http::StatusCode;
use http::header::{ACCEPT, CONTENT_TYPE, LOCATION};
use url::Url;

use crate::outcome;
use crate::redact::RedactedUrl;
use error::{InvalidInput, SubscribeError, SubscriptionMalformation};
use subscription::{FHIR_JSON, Subscribed, SubscriptionRequest, SubscriptionStatus};

/// The longest answer the subscriber reads: a `Subscription` or an
/// `OperationOutcome`, a few kilobytes at most.
const LIMIT: usize = 1 << 20;

/// A Patient Identity Subscriber bound to one Patient Identity Registry.
///
/// `Debug` shows the endpoint with its userinfo replaced by `***`, and leaves
/// out the HTTP client, whose default headers may hold a credential.
#[derive(Clone)]
pub struct PmirSubscriber {
    endpoint: Url,
    http: reqwest::Client,
    #[cfg(feature = "balp")]
    audit: Option<Arc<dyn crate::balp::AuditRecorder>>,
}

impl PmirSubscriber {
    /// Creates a subscriber for the Registry whose FHIR base URL is `base`.
    ///
    /// `http` carries the transport the caller chose: TLS, client
    /// certificates, default authorization headers, by which the Registry
    /// authorizes the subscriber (§2:3.94.5). Build it with
    /// `redirect::Policy::none()`, so the credentials go nowhere the base does
    /// not name.
    ///
    /// # Errors
    /// [`InvalidInput::Base`] when `base` is not an `http` or `https` URL
    /// without a query or a fragment.
    pub fn new(base: Url, http: reqwest::Client) -> Result<Self, InvalidInput> {
        let endpoint = crate::search::under_base(base, "Subscription").ok_or(InvalidInput::Base)?;
        Ok(Self {
            endpoint,
            http,
            #[cfg(feature = "balp")]
            audit: None,
        })
    }

    /// This subscriber, recording the audit record of every exchange with
    /// the Registry through `recorder` (§2:3.94.5.1, feature `balp`).
    ///
    /// An audited subscriber records each exchange before it returns,
    /// whatever its outcome; one whose record the recorder does not accept
    /// fails with [`SubscribeError::Audit`], and its answer is not used.
    #[cfg(feature = "balp")]
    #[must_use]
    pub fn audited(mut self, recorder: Arc<dyn crate::balp::AuditRecorder>) -> Self {
        self.audit = Some(recorder);
        self
    }

    /// Records the exchange `exchange` describes, when the subscriber is
    /// audited, and returns `result` unless the record was refused.
    #[cfg(feature = "balp")]
    async fn audit<T>(
        &self,
        exchange: impl FnOnce(&Result<T, SubscribeError>) -> crate::balp::Exchange,
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
        let response = self
            .http
            .get(url.clone())
            .header(ACCEPT, FHIR_JSON)
            .timeout(timeout)
            .send()
            .await
            .map_err(error::transport)?;
        let status = response.status();
        let media = media(&response);
        let body = read_body(response).await?;
        if matches!(status, StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND) {
            return Ok(subscription::Search::Unsupported);
        }
        if status != StatusCode::OK {
            return Err(rejected(status, media.as_deref(), &body));
        }
        Ok(subscription::Search::Found(subscription::listed(
            media.as_deref(),
            &body,
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
                        audit::SUBSCRIPTION_CREATE,
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
            .http
            .post(self.endpoint.clone())
            .header(CONTENT_TYPE, FHIR_JSON)
            .header(ACCEPT, FHIR_JSON)
            .body(body)
            .timeout(timeout)
            .send()
            .await
            .map_err(error::transport)?;
        let status = response.status();
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let media = media(&response);
        let body = read_body(response).await?;
        if status != StatusCode::CREATED {
            return Err(rejected(status, media.as_deref(), &body));
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
                        audit::SUBSCRIPTION_READ,
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
        let response = self
            .http
            .get(subscribed.location().clone())
            .header(ACCEPT, FHIR_JSON)
            .timeout(timeout)
            .send()
            .await
            .map_err(error::transport)?;
        let status = response.status();
        let media = media(&response);
        let body = read_body(response).await?;
        if status != StatusCode::OK {
            return Err(rejected(status, media.as_deref(), &body));
        }
        Ok(subscription::status(media.as_deref(), &body)?)
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
                        audit::SUBSCRIPTION_DELETE,
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
        let response = self
            .http
            .delete(subscribed.location().clone())
            .header(ACCEPT, FHIR_JSON)
            .timeout(timeout)
            .send()
            .await
            .map_err(error::transport)?;
        let status = response.status();
        let media = media(&response);
        let body = read_body(response).await?;
        if matches!(
            status,
            StatusCode::OK
                | StatusCode::ACCEPTED
                | StatusCode::NO_CONTENT
                | StatusCode::NOT_FOUND
                | StatusCode::GONE
        ) {
            return Ok(());
        }
        Err(rejected(status, media.as_deref(), &body))
    }
}

impl fmt::Debug for PmirSubscriber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PmirSubscriber")
            .field("endpoint", &RedactedUrl(self.endpoint.as_str()))
            .finish_non_exhaustive()
    }
}

/// The media type of `response`, when it names one.
fn media(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// The refusal of an answer with `status`, with the issue types of the
/// `OperationOutcome` it carries, if any.
fn rejected(status: StatusCode, media: Option<&str>, body: &[u8]) -> SubscribeError {
    SubscribeError::Rejected {
        status,
        issues: outcome::issues(media, body),
    }
}

/// Reads the answer's body, refusing one longer than [`LIMIT`].
async fn read_body(mut response: reqwest::Response) -> Result<Vec<u8>, SubscribeError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(error::transport)? {
        if body.len().saturating_add(chunk.len()) > LIMIT {
            return Err(SubscriptionMalformation::TooLarge { limit: LIMIT }.into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
