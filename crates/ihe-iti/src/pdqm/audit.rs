// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-78 and ITI-119 audit records of the Patient Demographics Consumer
//! (PDQm 3.2.0 §2:3.78.5.1 and §2:3.119.5.1.1) and of the Supplier
//! (§2:3.78.5.1.2, §2:3.119.5.1.2), feature `balp`.
//!
//! [`PdqmClient::audited`](super::PdqmClient::audited) gives the client a
//! [`AuditRecorder`](ihe_atna::balp::AuditRecorder); every search and every
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
//!
//! A search, a page or a match made for a user names them from their OAuth
//! token, in the user agent BALP's patterns leave optional.
//!
//! A Supplier records each search or match it [`Received`] by
//! [`supplier_records`], as the PDQm Query or Match Supplier audit profile
//! fixes it: one record per patient found, as BALP 1.1.4 §1:52.4.1.6
//! recommends, or one with no patient when none was.

use jiff::Timestamp;
use secrecy::SecretString;
use url::Url;

use super::error::PdqmError;
use crate::received::Received;
use ihe_atna::balp::{
    Coded, DESTINATION_ROLE, Direction, Entity, EventKind, Exchange, Outcome, Peer, REST, SEARCH,
    SOURCE_ROLE,
};
use ihe_atna::user::OnBehalfOf;

/// The `ITI-78` subtype.
pub const ITI_78: Coded = Coded::fixed(
    ihe_atna::balp::IHE_TRANSACTIONS,
    "ITI-78",
    "Mobile Patient Demographics Query",
);

/// The `ITI-119` subtype.
pub const ITI_119: Coded = Coded::fixed(
    ihe_atna::balp::IHE_TRANSACTIONS,
    "ITI-119",
    "Patient Demographics Match",
);

/// What the PDQm Query Consumer audit profile fixes.
#[must_use]
pub fn query_consumer() -> EventKind {
    EventKind {
        profile: "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Query.Audit.Consumer",
        event_type: REST,
        subtypes: vec![ITI_78, SEARCH],
        action: "E",
        client: SOURCE_ROLE,
        server: DESTINATION_ROLE,
    }
}

/// What the PDQm Match Consumer audit profile fixes; the `search` subtype
/// beside `ITI-119` is the profile's own example's.
#[must_use]
pub fn match_consumer() -> EventKind {
    EventKind {
        profile: "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Match.Audit.Consumer",
        event_type: REST,
        subtypes: vec![ITI_119, SEARCH],
        action: "E",
        client: SOURCE_ROLE,
        server: DESTINATION_ROLE,
    }
}

/// The audit record of one ITI-78 request `request` to the Supplier at
/// `base`, made for `on_behalf`, which ended in `result`.
pub(super) fn exchange<T>(
    base: &Url,
    request: SecretString,
    on_behalf: &OnBehalfOf,
    result: &Result<T, PdqmError>,
) -> Exchange {
    recorded(
        (query_consumer(), base),
        vec![Entity::Query(request)],
        on_behalf,
        result,
    )
}

/// The audit record of one ITI-119 request `request` to the Supplier at
/// `base`, naming the patient `patient` when the input identified one, made
/// for `on_behalf`, which ended in `result`.
pub(super) fn match_exchange<T>(
    base: &Url,
    (request, patient): (SecretString, Option<(&str, &SecretString)>),
    on_behalf: &OnBehalfOf,
    result: &Result<T, PdqmError>,
) -> Exchange {
    let mut entities = vec![Entity::Query(request)];
    if let Some((system, value)) = patient {
        entities.push(Entity::Patient {
            system: Some(system.to_owned()),
            namespace: None,
            value: value.clone(),
        });
    }
    recorded((match_consumer(), base), entities, on_behalf, result)
}

/// The record of `kind` for an exchange with the Supplier at `base` over
/// `entities`, made for `on_behalf`, which ended in `result`.
fn recorded<T>(
    (kind, base): (EventKind, &Url),
    entities: Vec<Entity>,
    on_behalf: &OnBehalfOf,
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
        outcome_description: None,
        direction: Direction::Sent {
            server: Peer::server(base),
        },
        on_behalf: on_behalf.clone(),
        entities,
        security: Vec::new(),
    }
}

/// What the PDQm Query Supplier audit profile fixes (§2:3.78.5.1.2).
#[must_use]
pub fn query_supplier() -> EventKind {
    EventKind {
        profile: "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Query.Audit.Supplier",
        event_type: REST,
        subtypes: vec![ITI_78, SEARCH],
        action: "E",
        client: SOURCE_ROLE,
        server: DESTINATION_ROLE,
    }
}

/// What the PDQm Match Supplier audit profile fixes (§2:3.119.5.1.2).
#[must_use]
pub fn match_supplier() -> EventKind {
    EventKind {
        profile: "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Match.Audit.Supplier",
        event_type: REST,
        subtypes: vec![ITI_119, SEARCH],
        action: "E",
        client: SOURCE_ROLE,
        server: DESTINATION_ROLE,
    }
}

/// The transaction a Supplier record is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupplierSearch {
    /// ITI-78, the Patient search.
    Query,
    /// ITI-119, `$match`.
    Match,
}

/// Returns the Supplier's records of the `search` `received`.
///
/// `query` is the raw request, each of `patients` a `Patient/{id}` reference
/// found, and the search ended in `outcome`: one record per patient, or one
/// with no patient when none was found (§2:3.78.5.1.2, §2:3.119.5.1.2).
#[must_use]
pub fn supplier_records(
    received: &Received,
    search: SupplierSearch,
    query: &SecretString,
    patients: &[String],
    outcome: Outcome,
) -> Vec<Exchange> {
    let kind = || match search {
        SupplierSearch::Query => query_supplier(),
        SupplierSearch::Match => match_supplier(),
    };
    if patients.is_empty() {
        return vec![received.exchange(kind(), outcome, vec![Entity::Query(query.clone())])];
    }
    patients
        .iter()
        .map(|patient| {
            received.exchange(
                kind(),
                outcome,
                vec![
                    Entity::Query(query.clone()),
                    Entity::PatientReference(SecretString::from(patient.clone())),
                ],
            )
        })
        .collect()
}
