// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-83 audit record of the Patient Identifier Cross-reference
//! Consumer (PIXm 3.1.0 §2:3.83.5.1.1), and the ITI-83 and ITI-104 records
//! of the Manager (§2:3.83.5.1.2, §2:3.104.5.1.2), feature `balp`.
//!
//! [`PixmClient::audited`](super::PixmClient::audited) gives the client a
//! [`AuditRecorder`](ihe_atna::balp::AuditRecorder); after every exchange it
//! records one [`Exchange`] as the PIXm Query Consumer audit profile fixes
//! it: built on the BALP Patient Query pattern, the `ITI-83` and `search`
//! subtypes, the request as sent in the query entity (its URL for a `GET`,
//! its request line, media type and `Parameters` body for a `POST`), and the
//! source identifier in the patient entity. An exchange made for a user
//! names them from their OAuth token, as §2:3.83.5.2.1 asks the record to
//! be augmented "following IHE-BALP".
//!
//! A Manager records each request it [`Received`]: an ITI-83 query by
//! [`manager_query`], as the PIXm Query Manager audit profile fixes it, and
//! an ITI-104 feed by [`manager_feed`], as the PIXm Feed Create, Update or
//! Delete Manager audit profile fixes it, with the fed identifier as the
//! patient and the person it is cross-referenced to as the data.

use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::FHIR_JSON;
use super::error::PixmError;
use super::identifier::SourceIdentifier;
use super::request::Request;
use crate::received::Received;
use ihe_atna::balp::{
    APPLICATION, CREATE, CUSTODIAN, Coded, DELETE, DESTINATION_ROLE, DOMAIN_RESOURCE, Direction,
    Entity, EventKind, Exchange, Outcome, Peer, REST, SEARCH, SOURCE_ROLE, SYSTEM_OBJECT, UPDATE,
    request_text,
};
use ihe_atna::user::OnBehalfOf;

/// The `ITI-83` subtype.
pub const ITI_83: Coded = Coded::fixed(
    ihe_atna::balp::IHE_TRANSACTIONS,
    "ITI-83",
    "Mobile Patient Identifier Cross-reference Query",
);

/// The `ITI-104` subtype.
pub const ITI_104: Coded = Coded::fixed(
    ihe_atna::balp::IHE_TRANSACTIONS,
    "ITI-104",
    "Patient Identity Feed FHIR",
);

/// What the PIXm Query Consumer audit profile fixes.
#[must_use]
pub fn query_consumer() -> EventKind {
    EventKind {
        profile: "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Query.Audit.Consumer",
        event_type: REST,
        subtypes: vec![ITI_83, SEARCH],
        action: "E",
        client: SOURCE_ROLE,
        server: DESTINATION_ROLE,
    }
}

/// The audit record of one ITI-83 exchange that asked the Manager at its
/// `operation` URL, `[base]/Patient/\$ihe-pix`, with the
/// request `request` about `source`, made for `on_behalf`, and ended in
/// `result`.
pub(super) fn exchange<T>(
    operation: &Url,
    (request, source): (&Request, &SourceIdentifier),
    on_behalf: &OnBehalfOf,
    result: &Result<T, PixmError>,
) -> Exchange {
    Exchange {
        kind: query_consumer(),
        recorded: Timestamp::now(),
        outcome: outcome(result),
        outcome_description: None,
        direction: Direction::Sent {
            server: Peer::server(&operation.join("..").unwrap_or_else(|_| operation.clone())),
        },
        on_behalf: on_behalf.clone(),
        entities: vec![Entity::Query(query(operation, request)), patient(source)],
        security: Vec::new(),
    }
}

/// The raw request BALP Query has the query entity record: the URL of a
/// `GET`, and the request line, media type and body of a `POST` to
/// `operation`, as the PDQm ITI-78 and ITI-119 records write a `POST`.
fn query(operation: &Url, request: &Request) -> SecretString {
    match request {
        Request::Get(url) => request_text(url),
        Request::Post(body) => SecretString::from(format!(
            "POST {}\nContent-Type: {FHIR_JSON}\n\n{}",
            request_text(operation).expose_secret(),
            body.expose_secret()
        )),
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

/// What the PIXm Query Manager audit profile fixes (§2:3.83.5.1.2).
#[must_use]
pub fn query_manager() -> EventKind {
    EventKind {
        profile: "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Query.Audit.Manager",
        event_type: REST,
        subtypes: vec![ITI_83, SEARCH],
        action: "E",
        client: SOURCE_ROLE,
        server: DESTINATION_ROLE,
    }
}

/// Returns the Manager's record of the ITI-83 query `received`, its raw
/// request `query`, about `source`, ended in `outcome` (§2:3.83.5.1.2).
#[must_use]
pub fn manager_query(
    received: &Received,
    query: SecretString,
    source: &SourceIdentifier,
    outcome: Outcome,
) -> Exchange {
    received.exchange(
        query_manager(),
        outcome,
        vec![Entity::Query(query), patient(source)],
    )
}

/// What an ITI-104 feed did to the cross-reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedAction {
    /// Add Patient: the identifier was new (§2:3.104.4.1).
    Create,
    /// Revise Patient or Resolve Duplicate Patient: the identifier was known
    /// (§2:3.104.4.1, §2:3.104.4.2).
    Update,
    /// Remove Patient: the identifier was removed (§2:3.104.4.3).
    Delete,
}

/// What the PIXm Feed Manager audit profile of `action` fixes.
///
/// A delete names the client as the application and the Manager as the
/// custodian, as BALP Patient Delete does, a create or an update the source
/// and the destination (§2:3.104.5.1.2).
#[must_use]
pub fn feed_manager(action: FeedAction) -> EventKind {
    let (profile, subtype, code, client, server) = match action {
        FeedAction::Create => (
            "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Feed.Create.Audit.Manager",
            CREATE,
            "C",
            SOURCE_ROLE,
            DESTINATION_ROLE,
        ),
        FeedAction::Update => (
            "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Feed.Update.Audit.Manager",
            UPDATE,
            "U",
            SOURCE_ROLE,
            DESTINATION_ROLE,
        ),
        FeedAction::Delete => (
            "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Feed.Delete.Audit.Manager",
            DELETE,
            "D",
            APPLICATION,
            CUSTODIAN,
        ),
    };
    EventKind {
        profile,
        event_type: REST,
        subtypes: vec![subtype, ITI_104],
        action: code,
        client,
        server,
    }
}

/// Returns the Manager's record of the ITI-104 feed `received`.
///
/// The feed did `action` to the identifier `fed`, cross-referenced to the
/// person `person` (a `Patient/{id}` reference) when one is known, and ended
/// in `outcome` (§2:3.104.5.1.2).
#[must_use]
pub fn manager_feed(
    received: &Received,
    action: FeedAction,
    fed: &SourceIdentifier,
    person: Option<&str>,
    outcome: Outcome,
) -> Exchange {
    let data = Entity::Resource {
        reference: person.map(str::to_owned),
        resource_type: "Patient",
        kind: SYSTEM_OBJECT,
        role: Some(DOMAIN_RESOURCE),
        name: None,
        query: None,
    };
    received.exchange(feed_manager(action), outcome, vec![data, patient(fed)])
}

/// The patient entity of `identifier`.
fn patient(identifier: &SourceIdentifier) -> Entity {
    Entity::Patient {
        system: Some(identifier.system().to_owned()),
        namespace: None,
        value: identifier.value().clone(),
    }
}
