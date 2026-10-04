// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-78 and ITI-119 audit records of the Patient Demographics Consumer
//! (PDQm 3.2.0 §2:3.78.5.1 and §2:3.119.5.1.1), feature `balp`.
//!
//! [`PdqmClient::audited`](super::PdqmClient::audited) gives the client a
//! [`AuditRecorder`](crate::balp::AuditRecorder); every search and every
//! page it asks for is recorded as one [`Exchange`] the PDQm Query Consumer
//! audit profile fixes: built on the BALP Query pattern, the `ITI-78` and
//! `search` subtypes, and the request as sent in the query entity. The
//! profile's patient entity is `0..1`, for a query that identifies one
//! patient; a demographics search identifies none on its own, so none is
//! written.
//!
//! Every match is recorded as the PDQm Match Consumer audit profile fixes it:
//! the same pattern with the `ITI-119` subtype, the request as sent in the
//! query entity, and the patient entity when the input names the patient by
//! one identifier, which the profile asks for when "one patient is explicitly
//! identified".

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

/// The `ITI-119` subtype.
pub const ITI_119: Coded = Coded {
    system: crate::balp::IHE_TRANSACTIONS,
    code: "ITI-119",
    display: "Patient Demographics Match",
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

/// What the PDQm Match Consumer audit profile fixes; the `search` subtype
/// beside `ITI-119` is the profile's own example's.
pub const MATCH_CONSUMER: EventKind = EventKind {
    profile: "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Match.Audit.Consumer",
    event_type: REST,
    subtypes: &[ITI_119, SEARCH],
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
    recorded(QUERY_CONSUMER, base, vec![Entity::Query(request)], result)
}

/// The audit record of one ITI-119 request `request` to the Supplier at
/// `base`, naming the patient `patient` when the input identified one, which
/// ended in `result`.
pub(super) fn match_exchange<T>(
    base: &Url,
    request: SecretString,
    patient: Option<(&str, &SecretString)>,
    result: &Result<T, PdqmError>,
) -> Exchange {
    let mut entities = vec![Entity::Query(request)];
    if let Some((system, value)) = patient {
        entities.push(Entity::Patient {
            system: system.to_owned(),
            value: value.clone(),
        });
    }
    recorded(MATCH_CONSUMER, base, entities, result)
}

/// The record of `kind` for an exchange with the Supplier at `base` over
/// `entities`, which ended in `result`.
fn recorded<T>(
    kind: EventKind,
    base: &Url,
    entities: Vec<Entity>,
    result: &Result<T, PdqmError>,
) -> Exchange {
    let outcome = match result {
        Ok(_) => Outcome::Success,
        Err(PdqmError::Timeout | PdqmError::Transport(_)) => Outcome::SeriousFailure,
        Err(_) => Outcome::MinorFailure,
    };
    Exchange {
        kind,
        recorded: Timestamp::now(),
        outcome,
        direction: Direction::Sent {
            server: Peer::server(base),
        },
        entities,
    }
}
