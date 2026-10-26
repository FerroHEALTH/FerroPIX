// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The PMIR subscriptions the Registry keeps (PMIR 1.6.0 §2:3.94.4.1.3).
//!
//! Each is the `Subscription` as the subscriber wrote it, with the Registry's
//! own id and status, its criteria and its channel endpoint beside it.
//!
//! A create activates the subscription at once, so it is stored `active`
//! (§2:3.94.4.1.3: "When the subscription has been activated, the
//! Subscription Resource status shall be changed to active"). An update with
//! status `off` disables it and one with `requested` activates it again
//! (§2:3.94.4.4). A delete marks the row deleted and keeps it (§2:3.94.4.5).

use fhir_types::r4::subscription::Subscription;
use ihe_iti::pmir::server::subscription::RequestedSubscription;
use uuid::Uuid;

use crate::store::{Store, StoreError};

/// What became of an update.
#[derive(Debug, Clone, PartialEq)]
pub enum Updated {
    /// The subscription, as it now stands.
    Done(Box<Subscription>),
    /// No live subscription has the id.
    NotFound,
    /// The update asked for a status the subscriber may not set: only
    /// `off` and `requested` (§2:3.94.4.4).
    Status,
}

/// Stores `requested` as a new, active subscription and returns it with its
/// id.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn create(
    store: &Store,
    requested: &RequestedSubscription,
) -> Result<Subscription, StoreError> {
    let id = Uuid::new_v4();
    let resource = stored(requested, id, "active");
    sqlx::query(
        "INSERT INTO pix.subscription (id, resource, status, criteria, endpoint)
         VALUES ($1, $2, 'active', $3, $4)",
    )
    .bind(id)
    .bind(sqlx::types::Json(&resource))
    .bind(criteria(requested))
    .bind(requested.endpoint().as_str())
    .execute(store.pool())
    .await?;
    Ok(resource)
}

/// Reads the live subscription `id`, `None` when there is none.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn read(store: &Store, id: Uuid) -> Result<Option<Subscription>, StoreError> {
    let found: Option<(sqlx::types::Json<Subscription>, String, Option<String>)> = sqlx::query_as(
        "SELECT resource, status, error FROM pix.subscription WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(store.pool())
    .await?;
    Ok(found.map(|(resource, status, error)| {
        let mut resource = resource.0;
        resource.status = status.as_str().into();
        resource.error = error.map(|error| error.as_str().into());
        resource
    }))
}

/// Replaces the live subscription `id` with `requested`, enabling or
/// disabling it by the status it states (§2:3.94.4.4).
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn update(
    store: &Store,
    id: Uuid,
    requested: &RequestedSubscription,
) -> Result<Updated, StoreError> {
    let status = match requested.status() {
        "off" => "off",
        "requested" => "active",
        _ => return Ok(Updated::Status),
    };
    let resource = stored(requested, id, status);
    let changed = sqlx::query(
        "UPDATE pix.subscription
         SET resource = $2, status = $3, criteria = $4, endpoint = $5, error = NULL
         WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(sqlx::types::Json(&resource))
    .bind(status)
    .bind(criteria(requested))
    .bind(requested.endpoint().as_str())
    .execute(store.pool())
    .await?
    .rows_affected();
    Ok(if changed == 0 {
        Updated::NotFound
    } else {
        Updated::Done(Box::new(resource))
    })
}

/// Deletes the live subscription `id`, returning whether there was one.
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn delete(store: &Store, id: Uuid) -> Result<bool, StoreError> {
    let changed = sqlx::query(
        "UPDATE pix.subscription SET deleted_at = now(), status = 'off'
         WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(store.pool())
    .await?
    .rows_affected();
    Ok(changed > 0)
}

/// One active subscription, as the ITI-93 sender reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Active {
    /// The subscription's id.
    pub id: Uuid,
    /// Its criteria, as written.
    pub criteria: String,
    /// Its channel endpoint.
    pub endpoint: String,
}

/// Returns every live subscription whose status is `active`, by id: a
/// disabled, failed or deleted one is sent nothing (§2:3.94.4.4,
/// §2:3.94.4.5).
///
/// # Errors
/// Returns [`StoreError::Query`] when the statement fails.
pub async fn active(store: &Store) -> Result<Vec<Active>, StoreError> {
    let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, criteria, endpoint FROM pix.subscription
         WHERE status = 'active' AND deleted_at IS NULL ORDER BY id",
    )
    .fetch_all(store.pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, criteria, endpoint)| Active {
            id,
            criteria,
            endpoint,
        })
        .collect())
}

/// The resource as stored: the subscriber's, with the Registry's id and
/// `status`.
fn stored(requested: &RequestedSubscription, id: Uuid, status: &str) -> Subscription {
    let mut resource = requested.resource().clone();
    resource.id = Some(id.to_string());
    resource.status = status.into();
    resource.error = None;
    resource
}

/// The criteria text the subscription states.
fn criteria(requested: &RequestedSubscription) -> String {
    requested
        .resource()
        .criteria
        .value
        .clone()
        .unwrap_or_default()
}
