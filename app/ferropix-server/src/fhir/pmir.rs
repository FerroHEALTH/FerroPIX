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

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::response::{IntoResponse as _, Response};
use axum::routing::{MethodFilter, MethodRouter, on};
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
use crate::fhir::wire;
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
    let call = Call::of_request(&parts, &face.fhir_base);
    let format = answer_format(&parts);
    let record = |id: Option<&str>, outcome| {
        records::subscription(&call, SubscriptionAction::Create, id, outcome)
    };
    let requested = match requested(&face, &parts, body).await {
        Ok(requested) => requested,
        Err(answer) => {
            return kept(
                &face.audit,
                record(None, Outcome::MinorFailure),
                answer.as_ref(),
                format,
            )
            .await;
        }
    };
    let mut transaction = match begin(&face, record(None, Outcome::SeriousFailure), format).await {
        Ok(transaction) => transaction,
        Err(refusal) => return *refusal,
    };
    match subscription::create(&mut transaction, &requested).await {
        Ok(created) => {
            let id = created.id.clone().unwrap_or_default();
            let location = face.location(&id);
            let committed = commit(&face, transaction, record(Some(&id), Outcome::Success)).await;
            match committed {
                Ok(()) => written(
                    &answer(StatusCode::CREATED, created),
                    format,
                    Some(&location),
                ),
                Err(refusal) => written(&refused(refusal), format, None),
            }
        }
        Err(error) => unstored(&face, &error, record(None, Outcome::SeriousFailure), format).await,
    }
}

/// `GET {base}/fhir/Subscription/{id}` (§2:3.94.4.3).
async fn read(face: Arc<Face>, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let call = Call::of_request(&parts, &face.fhir_base);
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
            send(
                &face,
                &parts,
                exchange,
                &refused(Refusal::NotFound),
                format,
                None,
            )
            .await
        }
        Err(error) => {
            tracing::error!(error = %ferrohealth_server::run::chain(&error), "an ITI-94 subscription could not be read");
            let exchange = records::subscription(
                &call,
                SubscriptionAction::Read,
                label.as_deref(),
                Outcome::SeriousFailure,
            );
            send(
                &face,
                &parts,
                exchange,
                &refused(Refusal::Unavailable),
                format,
                None,
            )
            .await
        }
    }
}

/// `PUT {base}/fhir/Subscription/{id}`: enable or disable (§2:3.94.4.4).
async fn update(face: Arc<Face>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let call = Call::of_request(&parts, &face.fhir_base);
    let format = answer_format(&parts);
    let id = id_of(&parts);
    let label = id.map(|id| id.to_string());
    let record = |id: Option<&str>, outcome| {
        records::subscription(&call, SubscriptionAction::Update, id, outcome)
    };
    let requested = match requested(&face, &parts, body).await {
        Ok(requested) => requested,
        Err(answer) => {
            let exchange = record(label.as_deref(), Outcome::MinorFailure);
            return kept(&face.audit, exchange, answer.as_ref(), format).await;
        }
    };
    let Some(id) = id else {
        let exchange = record(None, Outcome::MinorFailure);
        return kept(&face.audit, exchange, &refused(Refusal::NotFound), format).await;
    };
    let failed = record(label.as_deref(), Outcome::SeriousFailure);
    let mut transaction = match begin(&face, failed, format).await {
        Ok(transaction) => transaction,
        Err(refusal) => return *refusal,
    };
    let (outcome, refusal) = match subscription::update(&mut transaction, id, &requested).await {
        Ok(Updated::Done(updated)) => {
            let exchange = record(label.as_deref(), Outcome::Success);
            return match commit(&face, transaction, exchange).await {
                Ok(()) => written(&answer(StatusCode::OK, *updated), format, None),
                Err(refusal) => written(&refused(refusal), format, None),
            };
        }
        Ok(Updated::NotFound) => (Outcome::MinorFailure, refused(Refusal::NotFound)),
        Ok(Updated::Status) => (
            Outcome::MinorFailure,
            Answer::outcome(
                StatusCode::BAD_REQUEST,
                Severity::Error,
                IssueType::BusinessRule,
                "a subscriber enables a subscription with status requested and disables it with off",
            ),
        ),
        Err(error) => {
            tracing::error!(error = %ferrohealth_server::run::chain(&error), "an ITI-94 subscription could not be updated");
            (Outcome::SeriousFailure, refused(Refusal::Unavailable))
        }
    };
    drop(transaction);
    kept(
        &face.audit,
        record(label.as_deref(), outcome),
        &refusal,
        format,
    )
    .await
}

/// `DELETE {base}/fhir/Subscription/{id}` (§2:3.94.4.5).
async fn delete(face: Arc<Face>, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let call = Call::of_request(&parts, &face.fhir_base);
    let format = answer_format(&parts);
    let id = id_of(&parts);
    let label = id.map(|id| id.to_string());
    let record = |outcome| {
        records::subscription(&call, SubscriptionAction::Delete, label.as_deref(), outcome)
    };
    let Some(id) = id else {
        return kept(
            &face.audit,
            record(Outcome::MinorFailure),
            &refused(Refusal::NotFound),
            format,
        )
        .await;
    };
    let mut transaction = match begin(&face, record(Outcome::SeriousFailure), format).await {
        Ok(transaction) => transaction,
        Err(refusal) => return *refusal,
    };
    match subscription::delete(&mut transaction, id).await {
        Ok(true) => match commit(&face, transaction, record(Outcome::Success)).await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(refusal) => written(&refused(refusal), format, None),
        },
        Ok(false) => {
            drop(transaction);
            kept(
                &face.audit,
                record(Outcome::MinorFailure),
                &refused(Refusal::NotFound),
                format,
            )
            .await
        }
        Err(error) => unstored(&face, &error, record(Outcome::SeriousFailure), format).await,
    }
}

/// Inserts the record `exchange` of a subscription write in its
/// `transaction` and commits both (#742), or returns the refusal: the
/// record that could not be kept, or the store that could not commit.
async fn commit(
    face: &Face,
    mut transaction: sqlx::Transaction<'static, sqlx::Postgres>,
    exchange: ihe_atna::balp::Exchange,
) -> Result<(), Refusal> {
    if let Err(error) = face.audit.enlist(&mut transaction, &[exchange]).await {
        tracing::error!(error = %ferrohealth_server::run::chain(&error), "an ITI-94 write is refused: its audit record could not be kept");
        return Err(Refusal::Unaudited);
    }
    if let Err(error) = transaction.commit().await {
        tracing::error!(error = %ferrohealth_server::run::chain(&error), "an ITI-94 write could not be committed");
        return Err(Refusal::Unavailable);
    }
    face.audit.committed();
    Ok(())
}

/// Logs a store that could not take a subscription write, keeps its record
/// `exchange` and answers `503`.
async fn unstored(
    face: &Face,
    error: &crate::store::StoreError,
    exchange: ihe_atna::balp::Exchange,
    format: Format,
) -> Response {
    tracing::error!(error = %ferrohealth_server::run::chain(error), "an ITI-94 subscription could not be stored");
    kept(
        &face.audit,
        exchange,
        &refused(Refusal::Unavailable),
        format,
    )
    .await
}

/// Begins the transaction of a subscription write, or keeps its record
/// `failed` and answers `503` when the store lends no connection.
async fn begin(
    face: &Face,
    failed: ihe_atna::balp::Exchange,
    format: Format,
) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, Box<Response>> {
    match face.store.pool().begin().await {
        Ok(transaction) => Ok(transaction),
        Err(error) => Err(Box::new(
            unstored(face, &error.into(), failed, format).await,
        )),
    }
}

/// Keeps the record `exchange` of a write that changed nothing in the
/// audit outbox of `audit`, then sends `answer` in `format`; `503` when the
/// record could not be kept.
pub(super) async fn kept<T: Resource>(
    audit: &Audit,
    exchange: ihe_atna::balp::Exchange,
    answer: &Answer<T>,
    format: Format,
) -> Response {
    match audit.keep(&[exchange]).await {
        Ok(()) => written(answer, format, None),
        Err(error) => {
            tracing::error!(error = %ferrohealth_server::run::chain(&error), "a PMIR answer was withheld: its audit record could not be kept");
            written(&refused(Refusal::Unaudited), format, None)
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
    let format = wire::body_format(&parts.headers).map_err(|case| Box::new(case.answer()))?;
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
        Answer::created(subscription)
    } else {
        Answer::ok(subscription)
    }
}

/// A refusal the PMIR face answers with an `OperationOutcome`.
#[derive(Debug, Clone, Copy)]
pub(super) enum Refusal {
    /// A subscription that is not there (FHIR R4 http.html#read).
    NotFound,
    /// A store that could not serve the request.
    Unavailable,
    /// An answer withheld because its record was not stored.
    Unaudited,
}

/// The `OperationOutcome` answer of `refusal`.
pub(super) fn refused(refusal: Refusal) -> Answer<OperationOutcome> {
    let (status, issue, text) = match refusal {
        Refusal::NotFound => (
            StatusCode::NOT_FOUND,
            IssueType::NotFound,
            "no subscription has this id",
        ),
        Refusal::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            IssueType::Transient,
            "the store could not serve the request",
        ),
        Refusal::Unaudited => (
            StatusCode::SERVICE_UNAVAILABLE,
            IssueType::Transient,
            UNAUDITED,
        ),
    };
    Answer::outcome(status, Severity::Error, issue, text)
}

/// Stores the request's record on `audit`, in the request's place in the
/// spool.
pub(super) async fn record(
    audit: &Audit,
    parts: &Parts,
    exchange: &ihe_atna::balp::Exchange,
) -> Result<(), ()> {
    let stored = match parts.extensions.get::<Pending>() {
        Some(pending) => pending.record(audit, std::slice::from_ref(exchange)).await,
        None => audit.record(std::slice::from_ref(exchange)).await,
    };
    stored.map_err(|error| {
        tracing::error!(error = %ferrohealth_server::run::chain(&error), "a PMIR answer was withheld: its audit record could not be stored");
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
        Err(()) => written(&refused(Refusal::Unaudited), format, None),
    }
}

/// `answer` encoded in `format`.
pub(super) fn written<T: Resource>(
    answer: &Answer<T>,
    format: Format,
    location: Option<&str>,
) -> Response {
    let mut response = wire::send(answer, format);
    if response.status() == answer.status()
        && let Some(location) = location.and_then(|location| HeaderValue::from_str(location).ok())
    {
        response.headers_mut().insert(header::LOCATION, location);
    }
    response
}
