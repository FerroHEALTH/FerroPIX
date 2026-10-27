// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-94 routes of the FHIR face: the PMIR subscriptions.
//!
//! `POST {base}/fhir/Subscription` creates one (`201` with its `Location`),
//! `GET`, `PUT` and `DELETE` of `{base}/fhir/Subscription/{id}` read,
//! enable or disable, and delete it (PMIR 1.6.0 §2:3.94.4.1 to
//! §2:3.94.4.5, FHIR R4 http.html). The body is read by
//! [`ihe_iti::pmir::server::subscription`], in FHIR JSON or XML, and the
//! answer is written in the format `_format` or `Accept` asks. A channel
//! endpoint is `https`; plain `http` on a loopback host is admitted under
//! `[dev]` alone. Every request writes its BALP record (§2:3.94.5.1) before
//! the answer leaves, and a record that cannot be stored withholds the
//! answer with `503`.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::response::{IntoResponse as _, Response};
use axum::routing::{MethodFilter, MethodRouter, on};
use ferrohealth_smart::resource::Access;
use fhir_types::r4::operation_outcome::OperationOutcome;
use fhir_types::r4::subscription::Subscription;
use http::request::Parts;
use http::{HeaderValue, Method, StatusCode, header};
use ihe_atna::balp::Outcome;
use ihe_iti::outcome::IssueType;
use ihe_iti::pmir::server::subscription::RequestedSubscription;
use ihe_iti::server::answer::{Answer, Resource, Severity};
use ihe_iti::server::format::Format;
use url::Url;
use uuid::Uuid;

use crate::audit::Audit;
use crate::audit::records::{self, Call, SubscriptionAction};
use crate::audit::reserve::{Pending, UNAUDITED};
use crate::auth::table::Route;
use crate::config::section::is_loopback_url;
use crate::store::Store;
use crate::store::subscription::{self, Updated};

/// What every ITI-94 route reads.
#[derive(Debug, Clone)]
pub struct Face {
    /// The store.
    store: Store,
    /// The audit trail.
    audit: Audit,
    /// The FHIR base, `{public_base_url}/fhir`.
    fhir_base: Url,
    /// Whether the development profile admits a loopback `http` endpoint.
    development: bool,
}

impl Face {
    /// Returns the routes' state over `store`, recording to `audit`, under
    /// `public_base_url`, admitting a loopback `http` endpoint when
    /// `development`.
    #[must_use]
    pub fn new(store: Store, audit: Audit, public_base_url: &Url, development: bool) -> Self {
        let base = public_base_url.as_str().trim_end_matches('/');
        let fhir_base =
            Url::parse(&format!("{base}/fhir")).unwrap_or_else(|_| public_base_url.clone());
        Self {
            store,
            audit,
            fhir_base,
            development,
        }
    }

    /// The URL of the subscription `id`.
    fn location(&self, id: &str) -> String {
        format!(
            "{}/Subscription/{id}",
            self.fhir_base.as_str().trim_end_matches('/')
        )
    }
}

/// Returns the handler of `route` when it is an ITI-94 route, under
/// `filter`.
#[must_use]
pub fn handler(route: &Route, filter: MethodFilter, face: &Arc<Face>) -> Option<MethodRouter> {
    let face = Arc::clone(face);
    let handler = match (route.method.clone(), route.path) {
        (Method::POST, "/fhir/Subscription") => on(filter, move |request: Request| {
            create(Arc::clone(&face), request)
        }),
        (Method::GET, "/fhir/Subscription/{id}") => on(filter, move |request: Request| {
            read(Arc::clone(&face), request)
        }),
        (Method::PUT, "/fhir/Subscription/{id}") => on(filter, move |request: Request| {
            update(Arc::clone(&face), request)
        }),
        (Method::DELETE, "/fhir/Subscription/{id}") => on(filter, move |request: Request| {
            delete(Arc::clone(&face), request)
        }),
        _ => return None,
    };
    Some(handler)
}

/// `POST {base}/fhir/Subscription` (§2:3.94.4.1, §2:3.94.4.2).
async fn create(face: Arc<Face>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let call = call(&face.fhir_base, &parts);
    let format = answer_format(&parts);
    let requested = match requested(&face, &parts, body).await {
        Ok(requested) => requested,
        Err(answer) => {
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Create,
                None,
                Outcome::MinorFailure,
            );
            return send(&face, &parts, exchange, answer.as_ref(), format, None).await;
        }
    };
    match subscription::create(&face.store, &requested).await {
        Ok(created) => {
            let id = created.id.clone().unwrap_or_default();
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Create,
                Some(&id),
                Outcome::Success,
            );
            let location = face.location(&id);
            send(
                &face,
                &parts,
                exchange,
                &answer(StatusCode::CREATED, created),
                format,
                Some(&location),
            )
            .await
        }
        Err(error) => {
            tracing::error!(error = %crate::chain(&error), "an ITI-94 subscription could not be stored");
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Create,
                None,
                Outcome::SeriousFailure,
            );
            send(&face, &parts, exchange, &unavailable(), format, None).await
        }
    }
}

/// `GET {base}/fhir/Subscription/{id}` (§2:3.94.4.3).
async fn read(face: Arc<Face>, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let call = call(&face.fhir_base, &parts);
    let format = answer_format(&parts);
    let id = id_of(&parts);
    let found = match id {
        Some(id) => subscription::read(&face.store, id).await,
        None => Ok(None),
    };
    let label = id.map(|id| id.to_string());
    match found {
        Ok(Some(found)) => {
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Read,
                label.as_deref(),
                Outcome::Success,
            );
            send(
                &face,
                &parts,
                exchange,
                &answer(StatusCode::OK, found),
                format,
                None,
            )
            .await
        }
        Ok(None) => {
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Read,
                label.as_deref(),
                Outcome::MinorFailure,
            );
            send(&face, &parts, exchange, &not_found(), format, None).await
        }
        Err(error) => {
            tracing::error!(error = %crate::chain(&error), "an ITI-94 subscription could not be read");
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Read,
                label.as_deref(),
                Outcome::SeriousFailure,
            );
            send(&face, &parts, exchange, &unavailable(), format, None).await
        }
    }
}

/// `PUT {base}/fhir/Subscription/{id}`: enable or disable (§2:3.94.4.4).
async fn update(face: Arc<Face>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let call = call(&face.fhir_base, &parts);
    let format = answer_format(&parts);
    let id = id_of(&parts);
    let label = id.map(|id| id.to_string());
    let requested = match requested(&face, &parts, body).await {
        Ok(requested) => requested,
        Err(answer) => {
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Update,
                label.as_deref(),
                Outcome::MinorFailure,
            );
            return send(&face, &parts, exchange, answer.as_ref(), format, None).await;
        }
    };
    let Some(id) = id else {
        let exchange = records::subscription(
            &call,
            SubscriptionAction::Update,
            None,
            Outcome::MinorFailure,
        );
        return send(&face, &parts, exchange, &not_found(), format, None).await;
    };
    match subscription::update(&face.store, id, &requested).await {
        Ok(Updated::Done(updated)) => {
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Update,
                label.as_deref(),
                Outcome::Success,
            );
            send(
                &face,
                &parts,
                exchange,
                &answer(StatusCode::OK, *updated),
                format,
                None,
            )
            .await
        }
        Ok(Updated::NotFound) => {
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Update,
                label.as_deref(),
                Outcome::MinorFailure,
            );
            send(&face, &parts, exchange, &not_found(), format, None).await
        }
        Ok(Updated::Status) => {
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Update,
                label.as_deref(),
                Outcome::MinorFailure,
            );
            let refusal = Answer::outcome(
                StatusCode::BAD_REQUEST,
                Severity::Error,
                IssueType::BusinessRule,
                "a subscriber enables a subscription with status requested and disables it with off",
            );
            send(&face, &parts, exchange, &refusal, format, None).await
        }
        Err(error) => {
            tracing::error!(error = %crate::chain(&error), "an ITI-94 subscription could not be updated");
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Update,
                label.as_deref(),
                Outcome::SeriousFailure,
            );
            send(&face, &parts, exchange, &unavailable(), format, None).await
        }
    }
}

/// `DELETE {base}/fhir/Subscription/{id}` (§2:3.94.4.5).
async fn delete(face: Arc<Face>, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let call = call(&face.fhir_base, &parts);
    let format = answer_format(&parts);
    let id = id_of(&parts);
    let label = id.map(|id| id.to_string());
    let deleted = match id {
        Some(id) => subscription::delete(&face.store, id).await,
        None => Ok(false),
    };
    match deleted {
        Ok(true) => {
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Delete,
                label.as_deref(),
                Outcome::Success,
            );
            match record(&face.audit, &parts, &exchange).await {
                Ok(()) => StatusCode::NO_CONTENT.into_response(),
                Err(()) => written(&unaudited(), format, None),
            }
        }
        Ok(false) => {
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Delete,
                label.as_deref(),
                Outcome::MinorFailure,
            );
            send(&face, &parts, exchange, &not_found(), format, None).await
        }
        Err(error) => {
            tracing::error!(error = %crate::chain(&error), "an ITI-94 subscription could not be deleted");
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Delete,
                label.as_deref(),
                Outcome::SeriousFailure,
            );
            send(&face, &parts, exchange, &unavailable(), format, None).await
        }
    }
}

/// Reads the `Subscription` a create or an update carries, and holds its
/// endpoint to `https`, or to `http` on a loopback host under `[dev]`.
async fn requested(
    face: &Face,
    parts: &Parts,
    body: Body,
) -> Result<RequestedSubscription, Box<Answer<OperationOutcome>>> {
    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let Some(format) = Format::of_content_type(content_type) else {
        return Err(Box::new(Format::unsupported_media_type()));
    };
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return Err(Box::new(Answer::outcome(
            StatusCode::PAYLOAD_TOO_LARGE,
            Severity::Error,
            IssueType::TooLong,
            "the request body is larger than this server reads",
        )));
    };
    let requested =
        RequestedSubscription::read(format, &bytes).map_err(|error| Box::new(error.refusal()))?;
    let endpoint = requested.endpoint();
    let admitted = endpoint.scheme() == "https"
        || (face.development && endpoint.scheme() == "http" && is_loopback_url(endpoint));
    if !admitted {
        return Err(Box::new(Answer::outcome(
            StatusCode::BAD_REQUEST,
            Severity::Error,
            IssueType::Invalid,
            "the channel endpoint is not https",
        )));
    }
    Ok(requested)
}

/// The id a `Subscription/{id}` path names, `None` when it is no UUID.
fn id_of(parts: &Parts) -> Option<Uuid> {
    // NOTE: the Registry mints every subscription id as a UUID, so another id names none.
    parts
        .uri
        .path()
        .rsplit('/')
        .next()
        .and_then(|id| Uuid::try_parse(id).ok())
}

/// Who called and where, at the FHIR base `fhir_base`.
pub(super) fn call(fhir_base: &Url, parts: &Parts) -> Call {
    let access = parts
        .extensions
        .get::<Access>()
        .cloned()
        .unwrap_or(Access::Open);
    let remote = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip());
    Call::of(
        &access,
        remote,
        fhir_base,
        crate::http::exchanged_request_id(parts),
    )
}

/// The format the answer is written in: the one `_format` or `Accept` asks,
/// JSON when neither names one the face writes.
pub(super) fn answer_format(parts: &Parts) -> Format {
    let format = url::form_urlencoded::parse(parts.uri.query().unwrap_or_default().as_bytes())
        .find(|(name, _)| name == "_format")
        .map(|(_, value)| value.into_owned());
    let accept = parts
        .headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok());
    Format::negotiate(format.as_deref(), accept).unwrap_or(Format::Json)
}

/// An answer carrying `subscription`: `201` for a create, `200` otherwise.
fn answer(status: StatusCode, subscription: Subscription) -> Answer<Subscription> {
    if status == StatusCode::CREATED {
        ihe_iti::pmir::server::subscription::created(subscription)
    } else {
        ihe_iti::pmir::server::subscription::current(subscription)
    }
}

/// The `404` of a subscription that is not there (FHIR R4 http.html#read).
fn not_found() -> Answer<OperationOutcome> {
    Answer::outcome(
        StatusCode::NOT_FOUND,
        Severity::Error,
        IssueType::NotFound,
        "no subscription has this id",
    )
}

/// The `503` of a store that could not serve the request.
fn unavailable() -> Answer<OperationOutcome> {
    Answer::outcome(
        StatusCode::SERVICE_UNAVAILABLE,
        Severity::Error,
        IssueType::Transient,
        "the store could not serve the request",
    )
}

/// The `503` of an answer withheld because its record was not stored.
pub(super) fn unaudited() -> Answer<OperationOutcome> {
    Answer::outcome(
        StatusCode::SERVICE_UNAVAILABLE,
        Severity::Error,
        IssueType::Transient,
        UNAUDITED,
    )
}

/// Stores the request's record on `audit`, in the request's place in the
/// spool.
pub(super) async fn record(
    audit: &Audit,
    parts: &Parts,
    exchange: &ihe_atna::balp::Exchange,
) -> Result<(), ()> {
    let stored = match parts.extensions.get::<Pending>() {
        Some(pending) => pending.record(audit, exchange).await,
        None => audit.record(vec![exchange.clone()]).await,
    };
    stored.map_err(|error| {
        tracing::error!(error = %crate::chain(&error), "a PMIR answer was withheld: its audit record could not be stored");
    })
}

/// Stores the record `exchange`, then sends `answer` in `format`, with a
/// `Location` when one is named; `503` when the record could not be stored.
async fn send<T: Resource>(
    face: &Face,
    parts: &Parts,
    exchange: ihe_atna::balp::Exchange,
    answer: &Answer<T>,
    format: Format,
    location: Option<&str>,
) -> Response {
    match record(&face.audit, parts, &exchange).await {
        Ok(()) => written(answer, format, location),
        Err(()) => written(&unaudited(), format, None),
    }
}

/// `answer` encoded in `format`.
pub(super) fn written<T: Resource>(
    answer: &Answer<T>,
    format: Format,
    location: Option<&str>,
) -> Response {
    let Ok(bytes) = answer.encode(format) else {
        tracing::error!("a PMIR answer could not be encoded");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = (answer.status(), Body::from(bytes)).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.media_type()),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(location) = location.and_then(|location| HeaderValue::from_str(location).ok()) {
        headers.insert(header::LOCATION, location);
    }
    response
}
