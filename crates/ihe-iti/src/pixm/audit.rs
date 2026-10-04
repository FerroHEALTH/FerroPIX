// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-83 audit record of the Patient Identifier Cross-reference
//! Consumer (PIXm 3.1.0 §2:3.83.5.1.1), feature `balp`.
//!
//! [`PixmClient::audited`](super::PixmClient::audited) gives the client a
//! [`AuditRecorder`](crate::balp::AuditRecorder); after every exchange it
//! records one [`Exchange`] as the PIXm Query Consumer audit profile fixes
//! it: built on the BALP Patient Query pattern, the `ITI-83` and `search`
//! subtypes, the request as sent in the query entity, and the source
//! identifier in the patient entity.

use jiff::Timestamp;
use url::Url;

use super::error::PixmError;
use super::identifier::SourceIdentifier;
use crate::balp::{
    Coded, DESTINATION_ROLE, Direction, Entity, EventKind, Exchange, Outcome, Peer, REST, SEARCH,
    SOURCE_ROLE, request_text,
};

/// The `ITI-83` subtype.
pub const ITI_83: Coded = Coded {
    system: crate::balp::IHE_TRANSACTIONS,
    code: "ITI-83",
    display: "Mobile Patient Identifier Cross-reference Query",
};

/// What the PIXm Query Consumer audit profile fixes.
pub const QUERY_CONSUMER: EventKind = EventKind {
    profile: "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Query.Audit.Consumer",
    event_type: REST,
    subtypes: &[ITI_83, SEARCH],
    action: "E",
    client: SOURCE_ROLE,
    server: DESTINATION_ROLE,
};

/// The audit record of one ITI-83 exchange that asked the Manager at its
/// `operation` URL, `[base]/Patient/$ihe-pix`, with the
/// request `request` about `source`, and ended in `result`.
pub(super) fn exchange<T>(
    operation: &Url,
    request: &Url,
    source: &SourceIdentifier,
    result: &Result<T, PixmError>,
) -> Exchange {
    Exchange {
        kind: QUERY_CONSUMER,
        recorded: Timestamp::now(),
        outcome: outcome(result),
        direction: Direction::Sent {
            server: Peer::server(&operation.join("..").unwrap_or_else(|_| operation.clone())),
        },
        entities: vec![
            Entity::Query(request_text(request)),
            Entity::Patient {
                system: source.system().to_owned(),
                value: source.value().clone(),
            },
        ],
    }
}

/// How an exchange that ended in `result` is recorded: a cross-reference or
/// one of the profile's not-found answers is a success, an answer the
/// profile refuses a minor failure, and no answer a serious one.
fn outcome<T>(result: &Result<T, PixmError>) -> Outcome {
    match result {
        Ok(_) => Outcome::Success,
        Err(PixmError::Timeout | PixmError::Transport(_)) => Outcome::SeriousFailure,
        Err(_) => Outcome::MinorFailure,
    }
}
