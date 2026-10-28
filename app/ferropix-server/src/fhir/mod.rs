// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FHIR face under `{base}/fhir` and the operator API under
//! `{base}/operator`: every route of [`crate::auth::table`] the
//! configuration switches on, behind the guard.
//!
//! The capability statement, the SMART discovery document, the PIXm
//! Manager ([`pixm`]), the PDQm Supplier ([`pdqm`]), the PMIR Registry
//! ([`registry`] for ITI-93, [`pmir`] for ITI-94) and the operator API
//! ([`crate::operator`]) answer; a row no handler serves answers `501`, with
//! a `not-supported` issue on the FHIR face and a problem on the operator
//! API, so the guard in front of a new route is in place
//! and tested first. [`status`] is
//! the one table of every outcome a transaction answers, and [`wire`] puts
//! each in JSON or XML.

pub mod capability;
pub mod outcome;
pub mod pdqm;
pub mod pixm;
pub mod pmir;
pub mod registry;
pub mod status;
pub mod wire;

use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodFilter, on};
use ferrohealth_health::lifecycle::Lifecycle;
use ferrohealth_health::registry::Registry;
use ferropix_operator_api::health::{Dependencies, Outcome, Upstream, UpstreamKind};
use http::StatusCode;

use crate::audit::Audit;
use crate::audit::reserve::{self, Reserving};
use crate::auth::guard::{self, Confine};
use crate::auth::table::{self, Mount};
use crate::auth::{Discovery, Smart};
use crate::config::section::matching::Matching;
use crate::config::section::operator::Operator;
use crate::config::section::profiles::{Deprecated, Pdqm, Pixm, Pmir};
use crate::fhir::outcome::{IssueType, body, respond};
use crate::store::Store;

/// What the transactions of the FHIR face read and write: the store, the
/// `[matching]` section the feed and the scorer run with, and how the PDQm
/// Supplier answers a merged person.
#[derive(Debug, Clone)]
pub struct Backend {
    /// The cross-reference store.
    pub store: Store,
    /// The `[matching]` section, resolved.
    pub matching: Matching,
    /// `[pdqm] deprecated`.
    pub deprecated: Deprecated,
    /// The rest of the configuration the operator API shows.
    pub configured: crate::operator::Configured,
    /// Whether the development profile's `[dev]` table is set, which admits
    /// a loopback `http` subscription endpoint.
    pub development: bool,
}

/// Which switches of the configuration are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each field is one switch of the configuration, read once at start"
)]
pub struct Mounts {
    /// `[pixm] enabled`.
    pub pixm: bool,
    /// `[pixm] remove_patient`.
    pub remove_patient: bool,
    /// `[pdqm] enabled`.
    pub pdqm: bool,
    /// `[pmir] enabled`.
    pub pmir: bool,
    /// `[operator] enabled`.
    pub operator: bool,
}

impl Mounts {
    /// Returns every switch on.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            pixm: true,
            remove_patient: true,
            pdqm: true,
            pmir: true,
            operator: true,
        }
    }

    /// Returns the switches the `[pixm]`, `[pdqm]`, `[pmir]` and `[operator]`
    /// sections set.
    #[must_use]
    pub const fn of(pixm: &Pixm, pdqm: &Pdqm, pmir: &Pmir, operator: &Operator) -> Self {
        Self {
            pixm: pixm.enabled,
            remove_patient: pixm.remove_patient,
            pdqm: pdqm.enabled,
            pmir: pmir.enabled,
            operator: operator.enabled,
        }
    }

    /// Returns whether `mount` is switched on.
    #[must_use]
    pub const fn admits(self, mount: Mount) -> bool {
        match mount {
            Mount::Always => true,
            Mount::Pixm => self.pixm,
            Mount::PixmRemove => self.pixm && self.remove_patient,
            Mount::Pdqm => self.pdqm,
            Mount::Pmir => self.pmir,
            Mount::Operator => self.operator,
        }
    }
}

/// Where the server is mounted and where a client reaches it.
#[derive(Debug, Clone, Copy)]
pub struct Address<'a> {
    /// The base path every route is mounted under, empty for the root.
    pub base: &'a str,
    /// The URL a client reaches `{base}` at.
    pub public_base_url: &'a url::Url,
}

/// Returns the guarded routes `mounts` switches on for a server at
/// `address`.
///
/// Its transactions run over `backend`, and its dependency report over
/// `health` in `lifecycle`'s phase.
///
/// The router is meant to be nested under `base` with the health routes;
/// the guard reads the matched route templates under it.
pub fn routes(
    address: Address<'_>,
    mounts: Mounts,
    smart: &Arc<Smart>,
    health: &Registry,
    lifecycle: &Lifecycle,
    audit: &Audit,
    backend: &Backend,
) -> Router {
    let (base, public_base_url) = (address.base, address.public_base_url);
    let statement = Arc::new(capability::statement(public_base_url, smart, mounts));
    let configuration = smart.configuration().map(Arc::new);
    let manager = Arc::new(pixm::Manager::new(
        &backend.store,
        &backend.matching,
        audit,
        smart,
        base,
        public_base_url,
    ));
    let pdqm = Arc::new(pdqm::Face::new(
        Arc::new(crate::pdqm::Supplier::new(
            backend.store.clone(),
            &backend.matching,
            backend.deprecated,
            public_base_url,
        )),
        audit.clone(),
    ));
    let operator = Arc::new(crate::operator::Operator::new(
        backend.store.clone(),
        &backend.matching,
        backend.configured.clone(),
        audit.clone(),
        smart.sources().clone(),
        public_base_url,
    ));
    let pmir = Arc::new(pmir::Face::new(
        backend.store.clone(),
        audit.clone(),
        public_base_url,
        backend.development,
    ));
    let registry = Arc::new(registry::Registry::new(
        &backend.store,
        &backend.matching,
        audit,
        smart.sources(),
        public_base_url,
    ));
    let mut router = Router::new();
    for route in table::ROUTES
        .iter()
        .filter(|route| mounts.admits(route.mount))
    {
        let Ok(filter) = MethodFilter::try_from(route.method.clone()) else {
            continue;
        };
        if let Some(handler) = pdqm::handler(route, filter, &pdqm)
            .or_else(|| pmir::handler(route, filter, &pmir))
            .or_else(|| registry::handler(route, filter, &registry))
            .or_else(|| crate::operator::handler(route, filter, &operator))
        {
            router = router.route(route.path, handler);
            continue;
        }
        let handler = match route.interaction {
            "capabilities" => {
                let statement = Arc::clone(&statement);
                on(filter, move |request: Request| {
                    let statement = Arc::clone(&statement);
                    async move { metadata(&statement, &request) }
                })
            }
            "ITI-83" | "ITI-104" => pixm_route(&route.method, filter, &manager),
            "smart-configuration" => {
                let configuration = configuration.clone();
                on(filter, move || {
                    let configuration = configuration.clone();
                    async move { discovery(configuration.as_deref()) }
                })
            }
            "operator-dependencies" => {
                let (health, lifecycle) = (health.clone(), lifecycle.clone());
                let (audit, store) = (audit.clone(), backend.store.clone());
                on(filter, move || {
                    dependencies(
                        health.clone(),
                        lifecycle.clone(),
                        audit.clone(),
                        store.clone(),
                    )
                })
            }
            _ if route.mount == Mount::Operator => on(filter, pending_operator),
            _ => on(filter, pending_fhir),
        };
        router = router.route(route.path, handler);
    }
    router
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(Reserving::new(base, audit.clone())),
            reserve::layer,
        ))
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(Confine::new(base, Arc::clone(smart))),
            guard::confine,
        ))
        .layer(guard::layer(base, smart))
}

/// `GET {base}/fhir/metadata`: the statement in the format the request
/// negotiates, JSON or XML, or `406` (FHIR R4 http.html §3.1.0.1.10).
fn metadata(
    statement: &fhir_types::r4::capability_statement::CapabilityStatement,
    request: &Request,
) -> Response {
    match wire::negotiate(request.uri().query(), request.headers()) {
        Ok(format) => wire::send_resource(statement, format),
        Err(refused) => *refused,
    }
}

/// The handler of the PIXm route `method`: ITI-83 by `GET` or `POST`,
/// ITI-104's conditional update by `PUT` and its conditional delete by
/// `DELETE`.
fn pixm_route(
    method: &http::Method,
    filter: MethodFilter,
    manager: &Arc<pixm::Manager>,
) -> axum::routing::MethodRouter {
    let manager = Arc::clone(manager);
    match *method {
        http::Method::PUT => on(filter, move |request: Request| {
            let manager = Arc::clone(&manager);
            async move { manager.update(request).await }
        }),
        http::Method::DELETE => on(filter, move |request: Request| {
            let manager = Arc::clone(&manager);
            async move { manager.remove(request).await }
        }),
        _ => on(filter, move |request: Request| {
            let manager = Arc::clone(&manager);
            async move { manager.query(request).await }
        }),
    }
}

/// `GET {base}/fhir/.well-known/smart-configuration`: the SMART discovery
/// document, which "SHALL be JSON, regardless of `Accept` headers" (SMART
/// App Launch 2.2.0, Conformance, §Using Well-Known), or `404` on a surface
/// of Basic users alone.
fn discovery(configuration: Option<&Discovery>) -> Response {
    match configuration {
        Some(document) => body(StatusCode::OK, document, "application/json"),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `GET {base}/operator/dependencies`: the readiness report with the last
/// outcome of every upstream, behind the steward scope so an anonymous
/// reader learns no endpoint (FerroFED's decision A83; no specification
/// governs this: our own design), with every PMIR subscriber and the
/// outcome of its latest ITI-93 message, and every HL7 v2 consumer and the
/// outcome of its latest ITI-10 notification.
async fn dependencies(
    health: Registry,
    lifecycle: Lifecycle,
    audit: Audit,
    store: Store,
) -> Response {
    let subscribers = match crate::store::subscription::deliveries(&store).await {
        Ok(deliveries) => deliveries.into_iter().map(subscriber).collect(),
        Err(error) => {
            tracing::error!(error = %crate::chain(&error), "the PMIR subscribers could not be read");
            Vec::new()
        }
    };
    let consumers = consumers(&store).await;
    let report = Dependencies {
        readiness: health.readiness(&lifecycle).await,
        upstreams: audit
            .upstream()
            .into_iter()
            .chain(subscribers)
            .chain(consumers)
            .collect(),
    };
    body(StatusCode::OK, &report, "application/json")
}

/// The upstream entry of one PMIR subscriber: its endpoint without userinfo,
/// query or fragment, and the state of its latest message or its error.
fn subscriber(delivery: crate::store::subscription::LastDelivery) -> Upstream {
    let endpoint = url::Url::parse(&delivery.endpoint).map_or_else(
        |_| String::from("(an endpoint that is no URL)"),
        |mut url| {
            // NOTE: no specification governs this: our own design; userinfo and query may carry a credential, which the report never shows.
            let _no_user = url.set_username("");
            let _no_password = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        },
    );
    let last = match (delivery.error, delivery.last) {
        (Some(error), _) => Some(Outcome::down(error)),
        (None, Some(Ok(()))) => Some(Outcome::up()),
        (None, Some(Err(failure))) => Some(Outcome::down(failure)),
        (None, None) => None,
    };
    Upstream {
        kind: UpstreamKind::Subscriber,
        endpoint,
        last,
        at: delivery.at,
    }
}

/// The upstream entries of the configured HL7 v2 consumers, in
/// configuration order.
async fn consumers(store: &Store) -> Vec<Upstream> {
    let Some(settings) = store.consumers() else {
        return Vec::new();
    };
    let mut entries = Vec::with_capacity(settings.consumers.len());
    for configured in settings.consumers.iter() {
        let last = match crate::store::outbox::last_notification(store, &configured.name).await {
            Ok(last) => last,
            Err(error) => {
                tracing::error!(consumer = %configured.name, error = %crate::chain(&error), "the last ITI-10 notification could not be read");
                None
            }
        };
        entries.push(consumer(&configured.address, last));
    }
    entries
}

/// The upstream entry of one HL7 v2 consumer at `address`: `mllp://` and its
/// `host:port`, and the state of its latest notification. No specification
/// governs the `mllp` scheme: our own design names the transport.
fn consumer(address: &str, last: Option<crate::store::outbox::LastNotification>) -> Upstream {
    let (last, at) = match last {
        None => (None, None),
        Some(notified) => (
            Some(match notified.last {
                Ok(()) => Outcome::up(),
                Err(failure) => Outcome::down(failure),
            }),
            notified.at,
        ),
    };
    Upstream {
        kind: UpstreamKind::Consumer,
        endpoint: format!("mllp://{address}"),
        last,
        at,
    }
}

/// A row of the FHIR face that no handler serves: none today, so a row
/// added without its handler answers `501` rather than `404`.
async fn pending_fhir() -> Response {
    respond(
        StatusCode::NOT_IMPLEMENTED,
        IssueType::NotSupported,
        "FerroPIX does not serve this interaction yet",
        None,
    )
}

/// A row of the operator API that no handler serves: none today, so a row
/// added without its handler answers `501` rather than `404`.
async fn pending_operator() -> Response {
    crate::problem::blank(
        StatusCode::NOT_IMPLEMENTED,
        "FerroPIX does not serve this route yet",
    )
}

#[cfg(test)]
mod tests {
    use ferropix_operator_api::health::{Outcome, UpstreamKind};

    use super::{consumer, subscriber};
    use crate::store::outbox::LastNotification;
    use crate::store::subscription::LastDelivery;

    fn delivery(endpoint: &str) -> LastDelivery {
        LastDelivery {
            endpoint: endpoint.to_owned(),
            error: None,
            last: None,
            at: None,
        }
    }

    #[test]
    fn a_subscriber_entry_shows_no_credential_of_its_endpoint() {
        let entry = subscriber(delivery(
            "https://user:secret@subscriber.example.org/pmir?token=abc#part",
        ));
        assert_eq!(UpstreamKind::Subscriber, entry.kind);
        assert_eq!("https://subscriber.example.org/pmir", entry.endpoint);
        assert_eq!(None, entry.last, "before the first message");
        assert_eq!(
            "(an endpoint that is no URL)",
            subscriber(delivery("not a url")).endpoint
        );
    }

    #[test]
    fn a_subscriber_entry_states_its_error_before_its_last_outcome() {
        let mut delivered = delivery("https://subscriber.example.org/pmir");
        delivered.last = Some(Ok(()));
        delivered.at = Some(String::from("2026-10-26T12:00:00Z"));
        assert_eq!(Some(Outcome::up()), subscriber(delivered.clone()).last);
        delivered.last = Some(Err(String::from("the endpoint answered 503")));
        assert_eq!(
            Some(Outcome::down("the endpoint answered 503")),
            subscriber(delivered.clone()).last
        );
        delivered.error = Some(String::from("an ITI-93 message could not be delivered"));
        assert_eq!(
            Some(Outcome::down("an ITI-93 message could not be delivered")),
            subscriber(delivered).last
        );
    }

    #[test]
    fn a_consumer_entry_names_its_mllp_address_and_its_latest_outcome() {
        let before = consumer("consumer.example.org:2575", None);
        assert_eq!(UpstreamKind::Consumer, before.kind);
        assert_eq!("mllp://consumer.example.org:2575", before.endpoint);
        assert_eq!(None, before.last, "before the first notification");
        let refused = consumer(
            "consumer.example.org:2575",
            Some(LastNotification {
                last: Err(String::from("the consumer refused the connection")),
                at: None,
            }),
        );
        assert_eq!(
            Some(Outcome::down("the consumer refused the connection")),
            refused.last
        );
    }
}
