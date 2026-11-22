// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The place of a transaction's record, taken before the transaction runs.
//!
//! The layer in front of every audited read of the permission table takes
//! one place in the spool and leaves it on the request ([`Pending`]); the
//! read fills it with its records once it knows the outcome. A spool that
//! has no room refuses the request with `503`, a `transient` issue on the
//! FHIR face and the problem `audit-unavailable` on the operator API, before
//! its handler runs, so no read is answered that could not be audited, as
//! FerroFED's audit fails closed. Records the spool does not store within
//! its write timeout are withdrawn and the read is recorded and answered as
//! refused, so the trail and the client agree (#742). A write takes no place:
//! its record goes to the audit outbox in its transaction. A place the
//! handler leaves unfilled is given back when the request ends. No
//! specification governs the layer: our own design.

use std::sync::Arc;

use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use http::StatusCode;
use ihe_atna::atna::spool::reserve::Reservation;
use ihe_atna::atna::spool::withdraw::Withdrawable;
use ihe_atna::balp::{Exchange, Observer, Outcome};
use tokio::sync::Mutex;

use crate::audit::{Audit, Unrecorded, written};
use crate::auth::table;
use crate::outcome::{IssueType, respond};
use crate::problem::{self, Kind};

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

    /// Stores the records of `exchanges`, the first in this place, as one
    /// batch, or, when the spool does not store them within its write
    /// timeout, the records of the exchanges refused in their place.
    ///
    /// A refused exchange is recorded as its record with the outcome serious
    /// failure (FHIR R4 `audit-event-outcome` `8`): the answer is a `503`.
    ///
    /// # Errors
    ///
    /// Returns [`Unrecorded`] when a record cannot be written, the spool has
    /// no room for the rest, the records were withdrawn, or their batch
    /// could not be written.
    pub async fn record(self, exchanges: &[Exchange]) -> Result<(), Unrecorded> {
        let mut records = Vec::with_capacity(exchanges.len());
        for exchange in exchanges {
            records.push(withdrawable(exchange, &self.observer)?);
        }
        self.reservation
            .push_or_withdraw(records)
            .await
            .map_err(Unrecorded::Spool)
    }

    /// Stores the record of `exchange`, an outbound message already sent,
    /// in this place. A record the spool stores after its write timeout is
    /// still stored and delivered: the exchange happened, so its record is
    /// never withdrawn.
    ///
    /// # Errors
    ///
    /// Returns [`Unrecorded`] when the record cannot be written, the spool
    /// has no room for it, or it is not stored within the write timeout.
    pub async fn record_sent(self, exchange: &Exchange) -> Result<(), Unrecorded> {
        let bytes = written(exchange, &self.observer)?;
        self.reservation
            .push(bytes)
            .await
            .map_err(Unrecorded::Spool)
    }
}

/// The record of `exchange` as `observer` records it, with the record of
/// the exchange refused: the same record with the outcome serious failure.
fn withdrawable(exchange: &Exchange, observer: &Observer) -> Result<Withdrawable, Unrecorded> {
    let mut refused = exchange.clone();
    refused.outcome = Outcome::SeriousFailure;
    Ok(Withdrawable {
        message: written(exchange, observer)?,
        refusal: written(&refused, observer)?,
    })
}

/// The place of the request's record, which its handler fills.
#[derive(Debug, Clone)]
pub struct Pending(pub Arc<Mutex<Option<Place>>>);

impl Pending {
    /// Stores the records of `exchanges`, the first in the request's place.
    ///
    /// # Errors
    ///
    /// Returns [`Unrecorded`] as [`Place::record`] does; once the place is
    /// filled, a later call takes a place of its own from `audit`.
    pub async fn record(&self, audit: &Audit, exchanges: &[Exchange]) -> Result<(), Unrecorded> {
        let place = self.0.lock().await.take();
        match place {
            Some(place) => place.record(exchanges).await,
            None => audit.reserve()?.record(exchanges).await,
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
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| {
            let template = matched.as_str();
            template
                .strip_prefix(reserving.base.as_str())
                .unwrap_or(template)
        })
        .and_then(|template| table::find(request.method(), template))
        .filter(|route| route.audited() && !route.writes());
    let Some(route) = route else {
        return next.run(request).await;
    };
    match reserving.audit.reserve() {
        Ok(place) => {
            request
                .extensions_mut()
                .insert(Pending(Arc::new(Mutex::new(Some(place)))));
            next.run(request).await
        }
        Err(error) => {
            tracing::error!(error = %ferrohealth_server::run::chain(&error), "a request was refused: its audit record has no room in the spool");
            if route.mount == table::Mount::Operator {
                return problem::refuse(Kind::AuditUnavailable, UNAUDITED);
            }
            respond(
                StatusCode::SERVICE_UNAVAILABLE,
                IssueType::Transient,
                UNAUDITED,
                None,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::disallowed_types,
        reason = "the test seam: the records are read as JSON values"
    )]

    use ihe_atna::balp::{Direction, Exchange, NetworkAddress, Observer, Outcome};
    use secrecy::ExposeSecret as _;
    use serde_json::Value;

    use super::withdrawable;

    #[test]
    fn the_record_of_a_refused_read_differs_from_its_record_in_the_outcome_alone() {
        let observer = Observer {
            source_id: String::from("pix.example.org"),
            site: None,
            host: NetworkAddress::host("pix.example.org"),
            source_types: Vec::new(),
        };
        let exchange: Exchange = ihe_iti::pixm::audit::manager_query(
            &crate::audit::records::Call::of(
                &ferrohealth_smart::resource::Access::Open,
                None,
                &url::Url::parse("https://pix.example.org/fhir").expect("a URL"),
                Some("unit"),
            )
            .received(),
            crate::audit::records::raw_request(
                &url::Url::parse("https://pix.example.org/fhir/Patient/$ihe-pix").expect("a URL"),
                None,
            ),
            &ihe_iti::pixm::identifier::SourceIdentifier::new(
                "urn:oid:2.999.742",
                secrecy::SecretString::from(String::from("S-1")),
            )
            .expect("an identifier"),
            Outcome::Success,
        );
        assert!(matches!(exchange.direction, Direction::Received { .. }));
        let pair = withdrawable(&exchange, &observer).expect("both records write");
        let message: Value = serde_json::from_slice(pair.message.expose_secret()).expect("JSON");
        let mut refusal: Value =
            serde_json::from_slice(pair.refusal.expose_secret()).expect("JSON");
        assert_eq!("0", message["outcome"]);
        assert_eq!("8", refusal["outcome"], "FHIR R4 audit-event-outcome 8");
        refusal["outcome"] = message["outcome"].clone();
        assert_eq!(message, refusal, "the outcome is all that differs");
    }
}
