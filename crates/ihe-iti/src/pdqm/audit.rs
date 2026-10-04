// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-78 audit record of the Patient Demographics Consumer (PDQm 3.2.0
//! §2:3.78.5.1), feature `balp`.
//!
//! [`PdqmClient::audited`](super::PdqmClient::audited) gives the client a
//! [`AuditRecorder`](crate::balp::AuditRecorder); every search and every
//! page it asks for is recorded as one [`Exchange`] the PDQm Query Consumer
//! audit profile fixes: built on the BALP Query pattern, the `ITI-78` and
//! `search` subtypes, and the request as sent in the query entity. The
//! profile's patient entity is `0..1`, for a query that identifies one
//! patient; a demographics search identifies none on its own, so none is
//! written.

use jiff::Timestamp;
use secrecy::SecretString;
use url::Url;

use super::error::PdqmError;
use crate::balp::{
    Coded, DESTINATION_ROLE, Direction, Entity, EventKind, Exchange, Outcome, Peer, REST, SEARCH,
    SOURCE_ROLE,
};

/// The `ITI-78` subtype.
pub const ITI_78: Coded = Coded {
    system: crate::balp::IHE_TRANSACTIONS,
    code: "ITI-78",
    display: "Mobile Patient Demographics Query",
};

/// What the PDQm Query Consumer audit profile fixes.
pub const QUERY_CONSUMER: EventKind = EventKind {
    profile: "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Query.Audit.Consumer",
    event_type: REST,
    subtypes: &[ITI_78, SEARCH],
    action: "E",
    client: SOURCE_ROLE,
    server: DESTINATION_ROLE,
};

/// The audit record of one ITI-78 request `request` to the Supplier at
/// `base`, which ended in `result`.
pub(super) fn exchange<T>(
    base: &Url,
    request: SecretString,
    result: &Result<T, PdqmError>,
) -> Exchange {
    let outcome = match result {
        Ok(_) => Outcome::Success,
        Err(PdqmError::Timeout | PdqmError::Transport(_)) => Outcome::SeriousFailure,
        Err(_) => Outcome::MinorFailure,
    };
    Exchange {
        kind: QUERY_CONSUMER,
        recorded: Timestamp::now(),
        outcome,
        direction: Direction::Sent {
            server: Peer::server(base),
        },
        entities: vec![Entity::Query(request)],
    }
}
