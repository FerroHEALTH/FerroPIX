// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-90 and ITI-91 audit records of the Query Client and the Update
//! Client (mCSD 4.0.0 §2:3.90.5.1, §2:3.91.5.1), feature `balp`.
//!
//! [`McsdClient::audited`](super::client::McsdClient::audited) gives the
//! client a [`AuditRecorder`](crate::balp::AuditRecorder); it records one
//! [`Exchange`] per search ([`QUERY`], on the BALP Query pattern) and per
//! history ([`UPDATES`]), with the first request as sent in the query
//! entity. A care services search names no patient, so no patient entity is
//! written. The client is told of no user, so each record is the client's
//! own system's ([`OnBehalfOf::System`]) and names no user agent.

use jiff::Timestamp;
use url::Url;

use super::error::McsdError;
use crate::balp::{
    Coded, DESTINATION_ROLE, Direction, Entity, EventKind, Exchange, HISTORY_TYPE, Outcome, Peer,
    REST, SEARCH, SOURCE_ROLE, request_text,
};
use crate::user::OnBehalfOf;

/// The `ITI-90` subtype.
pub const ITI_90: Coded = Coded {
    system: crate::balp::IHE_TRANSACTIONS,
    code: "ITI-90",
    display: "Find Matching Care Services",
};

/// The `ITI-91` subtype.
pub const ITI_91: Coded = Coded {
    system: crate::balp::IHE_TRANSACTIONS,
    code: "ITI-91",
    display: "Request Care Services Updates",
};

/// What the Find Matching Care Services for Query audit profile fixes.
pub const QUERY: EventKind = EventKind {
    profile: "https://profiles.ihe.net/ITI/mCSD/StructureDefinition/IHE.mCSD.Audit.CareServices.Query",
    event_type: REST,
    subtypes: &[ITI_90, SEARCH],
    action: "E",
    client: SOURCE_ROLE,
    server: DESTINATION_ROLE,
};

/// What the Request Care Services Updates audit profile fixes.
pub const UPDATES: EventKind = EventKind {
    profile: "https://profiles.ihe.net/ITI/mCSD/StructureDefinition/IHE.mCSD.Audit.CareServices.Updates",
    event_type: REST,
    subtypes: &[HISTORY_TYPE, ITI_91],
    action: "E",
    client: SOURCE_ROLE,
    server: DESTINATION_ROLE,
};

/// The audit record of one search or history of `kind` that asked the
/// directory at `base` with the first request `request`, and ended in
/// `result`.
///
/// A search that follows `next` links is one transaction, recorded once
/// with the request that began it (no specification governs paging: our own
/// design).
pub(super) fn exchange<T>(
    kind: EventKind,
    base: &Url,
    request: &Url,
    result: &Result<T, McsdError>,
) -> Exchange {
    let outcome = match result {
        Ok(_) => Outcome::Success,
        Err(error) if error.answered() => Outcome::MinorFailure,
        Err(_) => Outcome::SeriousFailure,
    };
    Exchange {
        kind,
        recorded: Timestamp::now(),
        outcome,
        direction: Direction::Sent {
            server: Peer::server(base),
        },
        on_behalf: OnBehalfOf::System,
        entities: vec![Entity::Query(request_text(request))],
    }
}
