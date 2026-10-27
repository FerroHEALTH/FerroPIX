// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The XCPD Responding Gateway: ITI-55 answered over the cross-reference,
//! behind `[xcpd]`.
//!
//! `POST {base}/xcpd` reads one `PRPA_IN201305UV02` in its SOAP 1.2
//! envelope through `ihe-iti` (ITI TF-2 §3.55.4.1, §3.55.6) and answers the
//! `PRPA_IN201306UV02` of the case it settles (§3.55.4.2.3), on the same
//! exchange, or, when the request's `ReplyTo` names an endpoint, posted to
//! it later as Appendix V.5 describes; the gateway "shall support
//! Asynchronous Web Services Exchange" (§3.55.6):
//!
//! - An identifier of the query in a domain the store holds is looked up in
//!   the cross-reference: the persons found are Case 1 or Case 2, and an
//!   identifier the store has in two persons is Case 5 with
//!   `AnswerNotAvailable`, which the steward has to resolve.
//! - With no identifier found, the demographics are scored by the matcher as
//!   an ITI-119 input is: `certain` and `probable` persons are Case 1 or
//!   Case 2, `possible` ones alone are Case 3 asking for the scored
//!   attributes the query left out, and none is Case 4. `MinimumDegreeMatch`
//!   drops a person whose score, as a percentage, is below it
//!   (Table 3.55.4.1.2.2-1). The grades and the attributes asked for are our
//!   own design: IHE leaves matching to the gateway (§3.55.4.1.3.1).
//! - A person is answered with its identifiers in `[xcpd] domains`, and one
//!   with none is left out.
//! - A Deferred request is refused `NS250` (§3.55.4.1.3), a `ReplyTo` no
//!   `[xcpd] reply_endpoints` entry starts is a `Sender` fault, a query from an
//!   initiating community `[xcpd]` does not list is Case 5 with
//!   `AnswerNotAvailable`, a query targeted at another community is Case 5
//!   (§3.55.4.1.3.2), and a store that cannot be read is Case 5 with
//!   `InternalError`.
//!
//! A request that is no ITI-55 query is a SOAP 1.2 `Sender` fault, a
//! transmission error (§3.55.4.2.3). Every query writes the Responding
//! Gateway audit record before its answer leaves (§3.55.5.1.2), and an
//! answer whose record cannot be stored is replaced by Case 5. Transport
//! security, the mutual TLS of an ATNA Secure Node, is the deployment's, as
//! it is for the MLLP listener. No identifier or name reaches a log line.

pub mod person;

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::response::{IntoResponse as _, Response};
use axum::routing::post;
use http::{StatusCode, header};
use ihe_atna::balp::{NetworkAddress, Outcome as AuditOutcome, Peer};
use ihe_atna::user::OnBehalfOf;
use ihe_iti::xcpd::discovery::RequestedAttribute;
use ihe_iti::xcpd::error::DetectedIssue;
use ihe_iti::xcpd::responding::answer::{self, Blame, Candidate, MEDIA_TYPE, Outcome, Responder};
use ihe_iti::xcpd::responding::query::{self, Priority, ReceivedQuery};
use secrecy::{ExposeSecret as _, SecretString};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::audit::Audit;
use crate::audit::records::{self, Call, Identifier};
use crate::audit::reserve::Place;
use crate::config::section::matching::{BlockingKey, Matching};
use crate::config::section::xcpd::XcpdSettings;
use crate::feed::demographics::DemographicsError;
use crate::matching::scorer::{Grade, Scorer};
use crate::outbox::{Backoff, HttpChannel, Wake};
use crate::pdqm::PdqError;
use crate::pdqm::patient::{self, Person};
use crate::store::outbox::{self, Channel, Message};
use crate::store::xref::{self, Lookup};
use crate::store::{Store, StoreError};

/// The gateway's path under `{base}`.
pub const PATH: &str = "/xcpd";

/// The Responding Gateway over one store.
#[derive(Debug, Clone)]
pub struct Gateway {
    /// The store.
    store: Store,
    /// The scorer of the `[matching]` section.
    scorer: Scorer,
    /// The blocking keys, in the configured order.
    blocking: Vec<BlockingKey>,
    /// The system of the Master Patient Identification Domain.
    master: String,
    /// The `[xcpd]` section.
    settings: XcpdSettings,
    /// The audit trail.
    audit: Audit,
    /// The gateway's endpoint, as its records name it.
    endpoint: NetworkAddress,
    /// The client that posts an asynchronous answer.
    client: reqwest::Client,
    /// What wakes the outbox sender once an answer is written.
    wake: Wake,
}

impl Gateway {
    /// Returns the gateway over `store`, scoring with `matching`, answering
    /// as `settings` names it at `public_base_url`, recording to `audit`.
    ///
    /// # Errors
    /// Returns the client's error when the HTTP client that posts an
    /// asynchronous answer cannot be built, the configured client identity is
    /// no PEM certificate chain and private key it reads, or the trusted
    /// certificates are no PEM bundle it reads.
    pub fn new(
        store: &Store,
        matching: &Matching,
        settings: XcpdSettings,
        public_base_url: &url::Url,
        audit: &Audit,
        wake: Wake,
    ) -> Result<Self, reqwest::Error> {
        let master = public_base_url.as_str().trim_end_matches('/').to_owned();
        let endpoint = url::Url::parse(&format!("{master}{PATH}")).map_or_else(
            |_| NetworkAddress::uri(public_base_url),
            |url| NetworkAddress::uri(&url),
        );
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(settings.delivery.timeout);
        if let Some(pem) = &settings.delivery.trust {
            for certificate in reqwest::Certificate::from_pem_bundle(pem.as_bytes())? {
                builder = builder.add_root_certificate(certificate);
            }
        }
        if let Some(pem) = &settings.delivery.identity {
            builder = builder.identity(reqwest::Identity::from_pem(pem.expose().as_bytes())?);
        }
        let client = builder.build()?;
        Ok(Self {
            store: store.clone(),
            scorer: Scorer::new(matching),
            blocking: matching.blocking_keys.clone(),
            master,
            settings,
            audit: audit.clone(),
            endpoint,
            client,
            wake,
        })
    }

    /// Returns the outbox channel that delivers this gateway's asynchronous
    /// answers: its client and the retry waits of `[xcpd]`.
    #[must_use]
    pub fn channel(&self) -> HttpChannel {
        HttpChannel {
            client: self.client.clone(),
            backoff: Backoff {
                initial: self.settings.delivery.initial_backoff,
                max: self.settings.delivery.max_backoff,
            },
        }
    }

    /// Returns the responder every answer names.
    fn responder(&self) -> Responder {
        Responder {
            device: self.settings.device.clone(),
            community: self.settings.community.clone(),
        }
    }

    /// Answers one request of media type `media` and body `body`, from
    /// `peer`.
    ///
    /// A request whose `ReplyTo` names an endpoint is the asynchronous
    /// exchange of ITI TF-2 V.5: the answer is settled and written to the
    /// outbox, whose sender posts it to the endpoint, and only then is the
    /// request accepted with `202` and no envelope (WS-Addressing 1.0 SOAP
    /// Binding §5.2.2: the response "SHOULD NOT" be the outbound message of
    /// the same exchange), so an accepted answer survives a restart.
    async fn answer(
        self: &Arc<Self>,
        media: Option<&str>,
        body: &[u8],
        peer: Option<SocketAddr>,
    ) -> Response {
        let received = match query::read(media, body) {
            Ok(received) => received,
            Err(refusal) => {
                tracing::warn!(refusal = %refusal, "an ITI-55 request is refused");
                return fault(Blame::Sender, &refusal.to_string());
            }
        };
        let call = self.call(&received, peer);
        let reply = received
            .reply_to
            .as_deref()
            .map(|reply_to| self.reply_endpoint(reply_to));
        if matches!(reply, Some(None)) {
            tracing::warn!(
                "an ITI-55 request is refused: its ReplyTo is no configured reply endpoint"
            );
            return fault(
                Blame::Sender,
                "the ReplyTo endpoint is not one this gateway answers",
            );
        }
        let reply = reply.flatten();
        let Ok(place) = self.audit.reserve() else {
            tracing::error!("an ITI-55 query is refused: its audit record has no room");
            if reply.is_some() {
                return fault(Blame::Receiver, "the audit record has no room");
            }
            return self.envelope(
                &received,
                &Outcome::Unable(vec![DetectedIssue::ResponderBusy]),
            );
        };
        if received.priority == Priority::Deferred {
            // NOTE: ITI TF-2 §3.55.4.1.3: FerroPIX does not offer the Deferred Response Option.
            let exchange = record(&call, &received, &[], AuditOutcome::MinorFailure);
            if place.record(&exchange).await.is_err() {
                tracing::error!(
                    "an ITI-55 answer is withheld: its audit record could not be stored"
                );
                return fault(Blame::Receiver, "the audit record could not be stored");
            }
            return match answer::deferred_refused(&received, &self.responder()) {
                Ok(bytes) => soap(StatusCode::OK, bytes),
                Err(_) => fault(Blame::Receiver, "the answer could not be written"),
            };
        }
        let Some(endpoint) = reply else {
            return match self.concluded(&received, &call, place).await {
                Ok(bytes) => soap(StatusCode::OK, bytes),
                Err(_) => fault(Blame::Receiver, "the answer could not be written"),
            };
        };
        let Ok(bytes) = self.concluded(&received, &call, place).await else {
            return fault(Blame::Receiver, "the answer could not be written");
        };
        let message = Message {
            channel: Channel::Xcpd,
            destination: endpoint.to_string(),
            media_type: MEDIA_TYPE,
            payload: SecretString::from(String::from_utf8_lossy(&bytes).into_owned()),
            max_attempts: self.settings.delivery.attempts,
            subscription: None,
            consumer: None,
        };
        if let Err(error) = outbox::enqueue(self.store.pool(), self.store.sealer(), &message).await
        {
            tracing::error!(error = %crate::chain(&error), "an asynchronous ITI-55 answer could not be written to the outbox");
            return fault(Blame::Receiver, "the answer could not be queued");
        }
        self.wake.now();
        StatusCode::ACCEPTED.into_response()
    }

    /// Settles `received`, stores its audit record in `place`, and returns
    /// the answer: Case 5 when the record could not be stored.
    async fn concluded(
        &self,
        received: &ReceivedQuery,
        call: &Call,
        place: Place,
    ) -> std::io::Result<Vec<u8>> {
        let settled = self.settle(received).await;
        let patients: Vec<Identifier> = match &settled {
            Outcome::Matched(candidates) => candidates
                .iter()
                .map(|candidate| {
                    let (system, value) = person::patient_id(candidate);
                    Identifier { system, value }
                })
                .collect(),
            _ => Vec::new(),
        };
        let outcome = match &settled {
            Outcome::Unable(_) => AuditOutcome::MinorFailure,
            _ => AuditOutcome::Success,
        };
        let exchange = record(call, received, &patients, outcome);
        if place.record(&exchange).await.is_err() {
            tracing::error!("an ITI-55 answer is withheld: its audit record could not be stored");
            return answer::envelope(
                received,
                &self.responder(),
                &Outcome::Unable(vec![DetectedIssue::InternalError]),
            );
        }
        answer::envelope(received, &self.responder(), &settled)
    }

    /// Returns `reply_to` as a URL when it starts with a configured reply
    /// endpoint.
    fn reply_endpoint(&self, reply_to: &str) -> Option<url::Url> {
        // NOTE: a ReplyTo that is no URL names no endpoint this gateway can post to.
        let endpoint = url::Url::parse(reply_to).ok()?;
        self.settings
            .delivery
            .endpoints
            .iter()
            .any(|prefix| endpoint.as_str().starts_with(prefix.as_str()))
            .then_some(endpoint)
    }

    /// Settles the case of `received`.
    async fn settle(&self, received: &ReceivedQuery) -> Outcome {
        let admitted = received
            .sender_community
            .as_ref()
            .is_some_and(|community| self.settings.initiating.contains(community));
        if !admitted {
            tracing::warn!(
                "an ITI-55 query from an initiating community not configured is answered with Case 5"
            );
            return Outcome::Unable(vec![DetectedIssue::AnswerNotAvailable]);
        }
        if received
            .target_community
            .as_ref()
            .is_some_and(|target| *target != self.settings.community)
        {
            tracing::info!("an ITI-55 query targeted at another community is answered with Case 5");
            return Outcome::Unable(Vec::new());
        }
        match self.discover(received).await {
            Ok(settled) => settled,
            Err(error) => {
                tracing::error!(error = %crate::chain(&error), "an ITI-55 query could not be answered");
                Outcome::Unable(vec![DetectedIssue::InternalError])
            }
        }
    }

    /// Finds the persons `received` names, in one read-only snapshot.
    async fn discover(&self, received: &ReceivedQuery) -> Result<Outcome, XcpdError> {
        let mut snapshot = self.snapshot().await?;
        let mut persons: BTreeSet<Uuid> = BTreeSet::new();
        for identifier in &received.parameters.ids {
            let found = xref::lookup(
                &mut snapshot,
                self.store.sealer(),
                &identifier.authority().urn(),
                identifier.value().expose_secret(),
            )
            .await?;
            match found {
                Lookup::Found(reference) => {
                    persons.insert(reference.person.uuid());
                }
                Lookup::Ambiguous => {
                    snapshot.commit().await.map_err(store)?;
                    return Ok(Outcome::Unable(vec![DetectedIssue::AnswerNotAvailable]));
                }
                Lookup::NotFound | Lookup::UnknownDomain => {}
            }
        }
        let parameters = &received.parameters;
        let mut wanted: Vec<Uuid> = persons.into_iter().collect();
        let mut short = false;
        if wanted.is_empty() && !parameters.names.is_empty() && parameters.birth_time.is_some() {
            let matched = crate::pdqm::matching::run(
                &mut snapshot,
                self.store.sealer(),
                &self.scorer,
                &self.blocking,
                &person::input(parameters),
            )
            .await?;
            let minimum = parameters.minimum_degree_match.map(f64::from);
            let kept: Vec<_> = matched
                .found
                .into_iter()
                .filter(|found| minimum.is_none_or(|minimum| found.score * 100.0 >= minimum))
                .collect();
            wanted = kept
                .iter()
                .filter(|found| matches!(found.grade, Grade::Certain | Grade::Probable))
                .map(|found| found.person)
                .collect();
            short = wanted.is_empty() && kept.iter().any(|found| found.grade == Grade::Possible);
        }
        let candidates = self.candidates(&mut snapshot, &wanted).await?;
        snapshot.commit().await.map_err(store)?;
        if !candidates.is_empty() {
            return Ok(Outcome::Matched(candidates));
        }
        let asked = if short {
            requested(parameters)
        } else {
            Vec::new()
        };
        Ok(if asked.is_empty() {
            Outcome::NoMatch
        } else {
            Outcome::MoreAttributes(asked)
        })
    }

    /// Returns the `RegistrationEvent` patient of each person of `ids` that
    /// holds an identifier in `[xcpd] domains`, in order.
    async fn candidates(
        &self,
        snapshot: &mut Transaction<'static, Postgres>,
        ids: &[Uuid],
    ) -> Result<Vec<Candidate>, XcpdError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let persons = patient::persons(&mut *snapshot, ids).await?;
        let mut seen = BTreeSet::new();
        let active: Vec<Person> = ids
            .iter()
            .filter_map(|id| persons.get(id))
            .map(|person| Person {
                id: person.survivor.unwrap_or(person.id),
                survivor: None,
            })
            .filter(|person| seen.insert(person.id))
            .collect();
        let mut patients =
            patient::patients(&mut *snapshot, self.store.sealer(), &self.master, &active).await?;
        Ok(active
            .iter()
            .filter_map(|person| patients.remove(&person.id))
            .filter_map(|patient| {
                person::candidate(&patient, &self.settings.community, &self.settings.domains)
            })
            .collect())
    }

    /// Returns who called and where: the initiating gateway, named by its
    /// `ReplyTo` for an asynchronous exchange (ITI TF-2 §3.55.5.1.2) and by
    /// its community otherwise, at its network address.
    fn call(&self, received: &ReceivedQuery, peer: Option<SocketAddr>) -> Call {
        let who = received
            .reply_to
            .clone()
            .or_else(|| received.sender_community.as_ref().map(ToString::to_string))
            .or_else(|| {
                received
                    .sender_device
                    .as_ref()
                    .map(ihe_iti::xcpd::identifier::Oid::urn)
            })
            .unwrap_or_else(|| String::from("unknown"));
        Call {
            client: Peer {
                who,
                network: peer.map_or_else(
                    || self.endpoint.clone(),
                    |peer| NetworkAddress::Ip(peer.ip()),
                ),
            },
            on_behalf: OnBehalfOf::System,
            endpoint: self.endpoint.clone(),
            request_id: Some(received.message_id.clone()),
        }
    }

    /// Returns the answer of `settled` to `received`.
    fn envelope(&self, received: &ReceivedQuery, settled: &Outcome) -> Response {
        match answer::envelope(received, &self.responder(), settled) {
            Ok(bytes) => soap(StatusCode::OK, bytes),
            Err(_) => fault(Blame::Receiver, "the answer could not be written"),
        }
    }

    /// Opens a read-only transaction that sees one snapshot of the store.
    async fn snapshot(&self) -> Result<Transaction<'static, Postgres>, XcpdError> {
        let mut transaction = self.store.pool().begin().await.map_err(store)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(store)?;
        Ok(transaction)
    }
}

/// Returns the router of `POST {PATH}`.
pub fn routes(gateway: Gateway) -> Router {
    Router::new()
        .route(PATH, post(received))
        .with_state(Arc::new(gateway))
}

/// `POST {base}/xcpd`.
async fn received(State(gateway): State<Arc<Gateway>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| *address);
    let Ok(body) = axum::body::to_bytes(body, query::LIMIT).await else {
        tracing::warn!(
            "an ITI-55 request is refused: its body exceeds the limit or did not arrive"
        );
        return fault(
            Blame::Sender,
            "the request body exceeds the limit or did not arrive",
        );
    };
    let media = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    gateway.answer(media, &body, peer).await
}

/// Returns the record of `received`, answered with `patients`.
fn record(
    call: &Call,
    received: &ReceivedQuery,
    patients: &[Identifier],
    outcome: AuditOutcome,
) -> ihe_atna::balp::Exchange {
    let community = received.sender_community.as_ref().map(ToString::to_string);
    records::xcpd_query(
        call,
        SecretString::from(received.query_by_parameter.expose_secret().to_owned()),
        community.as_deref(),
        patients,
        outcome,
    )
}

/// Returns the scored attributes of the query that `parameters` leaves out,
/// which Case 3 asks for (ITI TF-2 §3.55.4.2.2.6).
fn requested(parameters: &query::Parameters) -> Vec<RequestedAttribute> {
    [
        (
            parameters.gender.is_none(),
            RequestedAttribute::AdministrativeGender,
        ),
        (
            parameters.addresses.is_empty(),
            RequestedAttribute::PatientAddress,
        ),
        (
            parameters.telecom.is_empty(),
            RequestedAttribute::PatientTelecom,
        ),
        (
            parameters.mothers_maiden_name.is_none(),
            RequestedAttribute::MothersMaidenName,
        ),
    ]
    .into_iter()
    .filter_map(|(missing, attribute)| missing.then_some(attribute))
    .collect()
}

/// Returns a SOAP answer of `status` with `bytes`.
fn soap(status: StatusCode, bytes: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        http::HeaderValue::from_static(MEDIA_TYPE),
    );
    response
}

/// Returns the SOAP 1.2 fault of `blame` with `reason`: `400` for the
/// sender's fault and `500` for the receiver's (SOAP 1.2 Part 2 §7.5.1.2).
fn fault(blame: Blame, reason: &str) -> Response {
    let status = match blame {
        Blame::Sender => StatusCode::BAD_REQUEST,
        Blame::Receiver => StatusCode::INTERNAL_SERVER_ERROR,
    };
    match answer::fault(blame, reason) {
        Ok(bytes) => soap(status, bytes),
        Err(_) => status.into_response(),
    }
}

/// Why a query could not be answered.
///
/// No variant carries a value from the request or the store.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum XcpdError {
    /// A statement on the store failed, or a sealed identifier did not open.
    #[error("the store could not be read")]
    Store(#[from] StoreError),
    /// Stored demographics are no `Patient`.
    #[error("the stored demographics could not be read")]
    Demographics(#[from] DemographicsError),
    /// The match could not be run.
    #[error("the match could not be run")]
    Match(#[from] PdqError),
}

/// Wraps a driver error.
fn store(source: sqlx::Error) -> XcpdError {
    XcpdError::Store(source.into())
}

#[cfg(test)]
mod tests {
    use ihe_iti::xcpd::discovery::RequestedAttribute;
    use ihe_iti::xcpd::responding::query::Parameters;

    use super::requested;

    #[test]
    fn the_vendored_soap_binding_holds_the_section_the_gateway_cites() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/specs/w3c-ws-addressing/ws-addr-soap-1.0.html"
        );
        let text = std::fs::read_to_string(path).expect("the vendored Recommendation reads");
        assert!(text.contains("5.2.2 SOAP 1.2"), "§5.2.2 is in the text");
        assert!(
            text.contains("SHOULD NOT"),
            "§5.2.2 says the response SHOULD NOT be the outbound message"
        );
    }

    #[test]
    fn case_three_asks_for_the_scored_attributes_the_query_left_out() {
        assert_eq!(
            vec![
                RequestedAttribute::AdministrativeGender,
                RequestedAttribute::PatientAddress,
                RequestedAttribute::PatientTelecom,
                RequestedAttribute::MothersMaidenName,
            ],
            requested(&Parameters::default())
        );
        let full = Parameters {
            gender: Some(String::from("F")),
            telecom: vec![String::from("tel:+31301234567")],
            ..Parameters::default()
        };
        assert_eq!(
            vec![
                RequestedAttribute::PatientAddress,
                RequestedAttribute::MothersMaidenName,
            ],
            requested(&full)
        );
    }
}
