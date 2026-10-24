// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The place of a transaction's record, taken before the transaction runs.
//!
//! The layer in front of every audited route of the permission table takes
//! one place in the spool and leaves it on the request ([`Pending`]); the
//! transaction fills it with its record once it knows the outcome. A spool
//! that has no room refuses the request with `503` and a `transient` issue
//! before its handler runs, so no transaction is made that could not be
//! audited, as FerroFED's audit fails closed. A place the handler leaves
//! unfilled is given back when the request ends. No specification governs
//! the layer: our own design.

use std::sync::Arc;

use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use http::StatusCode;
use ihe_atna::atna::spool::reserve::Reservation;
use ihe_atna::balp::{Exchange, Observer};
use tokio::sync::Mutex;

use crate::audit::{Audit, Unrecorded, written};
use crate::auth::table;
use crate::fhir::outcome::{IssueType, respond};

/// The diagnostics of a transaction refused because its record could not be
/// stored.
pub const UNAUDITED: &str =
    "the audit record of this request could not be stored, so the request was not processed";

/// The place of one record in the spool.
#[derive(Debug)]
pub struct Place {
    /// The place, taken under the spool's bounds.
    reservation: Reservation,
    /// The recording system.
    observer: Arc<Observer>,
}

impl Place {
    /// Returns the place `reservation` took, recorded as `observer`.
    pub(super) const fn new(reservation: Reservation, observer: Arc<Observer>) -> Self {
        Self {
            reservation,
            observer,
        }
    }

    /// Stores the record of `exchange` in this place.
    ///
    /// # Errors
    ///
    /// Returns [`Unrecorded`] when the record cannot be written, is larger
    /// than the place and the spool has no room for the rest, or is not
    /// stored within the spool's write timeout.
    pub async fn record(self, exchange: &Exchange) -> Result<(), Unrecorded> {
        let bytes = written(exchange, &self.observer)?;
        self.reservation
            .push(bytes)
            .await
            .map_err(Unrecorded::Spool)
    }
}

/// The place of the request's record, which its handler fills.
#[derive(Debug, Clone)]
pub struct Pending(pub Arc<Mutex<Option<Place>>>);

impl Pending {
    /// Stores the record of `exchange` in the request's place.
    ///
    /// # Errors
    ///
    /// Returns [`Unrecorded`] as [`Place::record`] does; a second record of
    /// one request takes a place of its own from `audit`.
    pub async fn record(&self, audit: &Audit, exchange: &Exchange) -> Result<(), Unrecorded> {
        let place = self.0.lock().await.take();
        match place {
            Some(place) => place.record(exchange).await,
            None => audit.reserve()?.record(exchange).await,
        }
    }
}

/// What the layer reads.
#[derive(Debug, Clone)]
pub struct Reserving {
    /// The base path every route is mounted under.
    base: String,
    /// The audit trail.
    audit: Audit,
}

impl Reserving {
    /// Returns the layer's state for routes under `base`.
    #[must_use]
    pub fn new(base: &str, audit: Audit) -> Self {
        Self {
            base: base.to_owned(),
            audit,
        }
    }
}

/// Takes the place of the record of an audited route's request, or refuses
/// the request with `503` when the spool has no room.
pub async fn layer(
    State(reserving): State<Arc<Reserving>>,
    mut request: Request,
    next: Next,
) -> Response {
    let audited = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| {
            let template = matched.as_str();
            template
                .strip_prefix(reserving.base.as_str())
                .unwrap_or(template)
        })
        .and_then(|template| table::find(request.method(), template))
        .is_some_and(table::Route::audited);
    if !audited {
        return next.run(request).await;
    }
    match reserving.audit.reserve() {
        Ok(place) => {
            request
                .extensions_mut()
                .insert(Pending(Arc::new(Mutex::new(Some(place)))));
            next.run(request).await
        }
        Err(error) => {
            tracing::error!(error = %crate::chain(&error), "a request was refused: its audit record has no room in the spool");
            respond(
                StatusCode::SERVICE_UNAVAILABLE,
                IssueType::Transient,
                UNAUDITED,
                None,
            )
        }
    }
}
