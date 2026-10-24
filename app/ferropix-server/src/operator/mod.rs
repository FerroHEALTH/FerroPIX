// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The operator API under `{base}/operator`, the routes the console reads
//! (`docs/architecture.md` §12.4), each behind the product scope
//! [`crate::auth::table`] names.
//!
//! - [`overview`]: the dashboard.
//! - [`search`]: by identifier and by demographics, both posted.
//! - [`person`]: the person with its identifiers, records, history and
//!   pseudonyms, which records a `view` action.
//! - [`domains`]: the identifier domains.
//! - [`matching`]: the matching configuration in effect.
//! - [`actions`]: the audit trail and its export.
//!
//! Every answer is a [`ferropix_operator_api`] document in JSON, never kept
//! by a cache (`Cache-Control: no-store`); a refusal is an
//! `OperationOutcome` whose diagnostics quote nothing from the request. A
//! route that reads a person, a search or an export writes its BALP record
//! into the place the audit layer reserved before it answers, and a record
//! that cannot be stored refuses the answer with `503`. No log line, span or
//! error carries an identifier, a name or a query value. No specification
//! governs the API: our own design.

pub mod actions;
pub mod demographics;
pub mod domains;
pub mod matching;
pub mod overview;
pub mod person;
pub mod search;
pub mod wire;

use std::sync::Arc;

use axum::extract::Request;
use axum::routing::{MethodFilter, MethodRouter, on};
use ferropix_operator_api::patient::{Channel, DomainKind, Grade};
use http::Method;
use sqlx::{Postgres, Transaction};
use url::Url;

use crate::audit::Audit;
use crate::auth::table::Route;
use crate::config::section::matching::{BlockingKey, Matching};
use crate::feed::demographics::DemographicsError;
use crate::matching::scorer::Scorer;
use crate::store::seal::{IdentifierValue, SealError, Sealer};
use crate::store::{Store, StoreError};

/// What every operator route reads.
#[derive(Debug, Clone)]
pub struct Operator {
    /// The cross-reference store.
    store: Store,
    /// The `[matching]` section, resolved.
    matching: Matching,
    /// The scorer of that section.
    scorer: Scorer,
    /// The blocking keys, in the configured order.
    blocking: Vec<BlockingKey>,
    /// The audit trail.
    audit: Audit,
    /// The operator API's base, `{public_base_url}/operator`, which the audit
    /// records name as the endpoint.
    endpoint: Url,
}

impl Operator {
    /// Returns the routes' state over `store`, with `matching`, recording to
    /// `audit`, for a server reached at `public_base_url`.
    #[must_use]
    pub fn new(store: Store, matching: &Matching, audit: Audit, public_base_url: &Url) -> Self {
        let master = public_base_url.as_str().trim_end_matches('/').to_owned();
        let endpoint =
            Url::parse(&format!("{master}/operator")).unwrap_or_else(|_| public_base_url.clone());
        Self {
            store,
            matching: matching.clone(),
            scorer: Scorer::new(matching),
            blocking: matching.blocking_keys.clone(),
            audit,
            endpoint,
        }
    }
}

/// Why an operator route could not answer.
///
/// No variant carries a value from the request or the store.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OperatorError {
    /// A statement on the store failed, or a sealed value did not open.
    #[error("the store could not be read")]
    Store(#[from] StoreError),
    /// Stored demographics are no `Patient`.
    #[error("the stored demographics could not be read")]
    Demographics(#[from] DemographicsError),
    /// The store holds a value its schema does not admit.
    #[error("the store holds a {0} its schema does not admit")]
    Unadmitted(&'static str),
    /// The scorer could not read the store.
    #[error("the match could not be run")]
    Match(#[from] crate::pdqm::PdqError),
}

impl From<sqlx::Error> for OperatorError {
    fn from(source: sqlx::Error) -> Self {
        Self::Store(source.into())
    }
}

/// Returns the handler of `route` when the operator API serves it, under
/// `filter`.
#[must_use]
pub fn handler(
    route: &Route,
    filter: MethodFilter,
    operator: &Arc<Operator>,
) -> Option<MethodRouter> {
    let operator = Arc::clone(operator);
    let handler = match (route.method.clone(), route.path) {
        (Method::GET, "/operator/overview") => on(filter, move || {
            let operator = Arc::clone(&operator);
            async move { overview::answer(&operator).await }
        }),
        (Method::POST, "/operator/search/identifier") => on(filter, move |request: Request| {
            let operator = Arc::clone(&operator);
            async move { search::by_identifier(&operator, request).await }
        }),
        (Method::POST, "/operator/search/demographics") => on(filter, move |request: Request| {
            let operator = Arc::clone(&operator);
            async move { search::by_demographics(&operator, request).await }
        }),
        (Method::GET, "/operator/persons/{id}") => on(filter, move |request: Request| {
            let operator = Arc::clone(&operator);
            async move { person::answer(&operator, request).await }
        }),
        (Method::GET, "/operator/domains") => on(filter, move |request: Request| {
            let operator = Arc::clone(&operator);
            async move { domains::list(&operator, request).await }
        }),
        (Method::GET, "/operator/domains/{id}") => on(filter, move |request: Request| {
            let operator = Arc::clone(&operator);
            async move { domains::one(&operator, request).await }
        }),
        (Method::GET, "/operator/matching") => on(filter, move || {
            let operator = Arc::clone(&operator);
            async move { matching::answer(&operator) }
        }),
        (Method::GET, "/operator/actions") => on(filter, move |request: Request| {
            let operator = Arc::clone(&operator);
            async move { actions::list(&operator, request).await }
        }),
        (Method::POST, "/operator/actions/export") => on(filter, move |request: Request| {
            let operator = Arc::clone(&operator);
            async move { actions::export(&operator, request).await }
        }),
        _ => return None,
    };
    Some(handler)
}

/// Opens one read-only snapshot, so every statement of an answer reads the
/// same state.
///
/// # Errors
///
/// Returns [`OperatorError::Store`] when the transaction cannot begin.
pub async fn snapshot(store: &Store) -> Result<Transaction<'static, Postgres>, OperatorError> {
    let mut transaction = store.pool().begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *transaction)
        .await?;
    Ok(transaction)
}

/// Returns a count the store answered, which is never negative.
#[must_use]
pub const fn count(stored: i64) -> u64 {
    stored.unsigned_abs()
}

/// Returns the grade the store spells `stored`.
///
/// # Errors
///
/// Returns [`OperatorError::Unadmitted`] for a grade the schema refuses.
pub fn grade(stored: &str) -> Result<Grade, OperatorError> {
    match stored {
        "certain" => Ok(Grade::Certain),
        "probable" => Ok(Grade::Probable),
        "possible" => Ok(Grade::Possible),
        _ => Err(OperatorError::Unadmitted("candidate grade")),
    }
}

/// Returns the channel the store spells `stored`.
///
/// # Errors
///
/// Returns [`OperatorError::Unadmitted`] for a channel the schema refuses.
pub fn channel(stored: &str) -> Result<Channel, OperatorError> {
    match stored {
        "fhir" => Ok(Channel::Fhir),
        "v2" => Ok(Channel::V2),
        "pmir" => Ok(Channel::Pmir),
        _ => Err(OperatorError::Unadmitted("channel")),
    }
}

/// Returns the domain kind the store spells `stored`.
///
/// # Errors
///
/// Returns [`OperatorError::Unadmitted`] for a kind the schema refuses.
pub fn domain_kind(stored: &str) -> Result<DomainKind, OperatorError> {
    match stored {
        "local" => Ok(DomainKind::Local),
        "authoritative" => Ok(DomainKind::Authoritative),
        "master" => Ok(DomainKind::Master),
        _ => Err(OperatorError::Unadmitted("domain kind")),
    }
}

/// Opens the identifier a row holds sealed in a sensitive domain or plain in
/// another, under `keys`.
///
/// # Errors
///
/// Returns [`OperatorError::Store`] when the sealed value does not open or
/// the row holds neither form.
pub fn opened(
    keys: &Sealer,
    domain: uuid::Uuid,
    sealed: Option<&[u8]>,
    plain: Option<&str>,
) -> Result<IdentifierValue, OperatorError> {
    match (sealed, plain) {
        (Some(sealed), _) => Ok(keys.open(domain, sealed).map_err(StoreError::Seal)?),
        (None, Some(plain)) => IdentifierValue::new(plain)
            .ok_or(OperatorError::Store(StoreError::Seal(SealError::Malformed))),
        (None, None) => Err(OperatorError::Store(StoreError::Seal(SealError::Malformed))),
    }
}

/// What one steward action row names.
#[derive(Debug, Clone, Default)]
pub struct Recorded {
    /// The action's kind.
    pub kind: &'static str,
    /// Who acted.
    pub actor: String,
    /// The person the action concerns.
    pub person: Option<uuid::Uuid>,
    /// The record the action concerns.
    pub record: Option<uuid::Uuid>,
    /// The reason given.
    pub reason: Option<String>,
    /// The request the action was made in.
    pub request_id: Option<String>,
}

/// Writes the steward action row `recorded` and returns its id.
///
/// # Errors
///
/// Returns [`OperatorError::Store`] when the statement fails.
pub async fn record_action<'c, E>(
    executor: E,
    recorded: &Recorded,
) -> Result<uuid::Uuid, OperatorError>
where
    E: sqlx::PgExecutor<'c>,
{
    let id = sqlx::query_scalar(
        "INSERT INTO pix.steward_action (kind, actor, person_id, record_id, reason, request_id)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id",
    )
    .bind(recorded.kind)
    .bind(&recorded.actor)
    .bind(recorded.person)
    .bind(recorded.record)
    .bind(recorded.reason.as_deref())
    .bind(recorded.request_id.as_deref())
    .fetch_one(executor)
    .await?;
    Ok(id)
}
