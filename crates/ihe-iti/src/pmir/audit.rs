// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-93 and ITI-94 audit records (PMIR 1.6.0 §2:3.93.5.1,
//! §2:3.94.5.1), feature `balp`.
//!
//! - [`PmirSubscriber::audited`](super::PmirSubscriber::audited) records
//!   each subscription create, read and delete as the Subscriber's
//!   [`subscription_create()`], [`subscription_read()`] and
//!   [`subscription_delete()`] audit profiles fix them, and the search by
//!   channel endpoint, which ITI-94 does not define, on the BALP Query
//!   pattern ([`subscription_search()`]).
//! - [`received`] writes the audit record of one ITI-93 message a Patient
//!   Identity Consumer received, as the [`feed_kind()`] audit profile fixes it: the
//!   Registry as the source, the Consumer's channel endpoint as the
//!   destination, one patient entity per Patient Master Identity the
//!   message names, and the message's `MessageHeader`.
//!
//! A subscription names an identifier system at most, never a patient, so
//! no ITI-94 record carries a patient entity (each profile's
//! `entity:patient` is `0..*`).
//!
//! Every exchange is the system's own ([`OnBehalfOf::System`]): a
//! subscription is the Consumer's, and a feed message the Registry sends of
//! its own accord, so no record names a user agent.

use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::error::SubscribeError;
use super::feed::{Event, Feed, PatientIdentity};
use ihe_atna::balp::{
    APPLICATION, CREATE, CUSTODIAN, Coded, DELETE, DESTINATION_ROLE, DOMAIN_RESOURCE, Direction,
    Entity, EventKind, Exchange, NetworkAddress, Outcome, Peer, READ, REST, SEARCH, SOURCE_ROLE,
    SYSTEM_OBJECT, request_text,
};
use ihe_atna::user::OnBehalfOf;

/// The `ITI-93` subtype.
pub const ITI_93: Coded = Coded::fixed(
    ihe_atna::balp::IHE_TRANSACTIONS,
    "ITI-93",
    "Mobile Patient Identity Feed",
);

/// The `ITI-94` subtype.
pub const ITI_94: Coded = Coded::fixed(
    ihe_atna::balp::IHE_TRANSACTIONS,
    "ITI-94",
    "Subscribe to Patient Updates",
);

/// The `type` of the Feed audit profile: DICOM `110110`.
pub const PATIENT_RECORD: Coded = Coded::fixed(
    "http://dicom.nema.org/resources/ontology/DCM",
    "110110",
    "Patient Record",
);

/// The `type` of the Feed audit profile's message entity.
pub const MESSAGE_HEADER: Coded = Coded::fixed(
    "http://hl7.org/fhir/resource-types",
    "MessageHeader",
    "MessageHeader",
);

/// What the Feed audit profile fixes.
#[must_use]
pub fn feed_kind() -> EventKind {
    EventKind {
        profile: "https://profiles.ihe.net/ITI/PMIR/StructureDefinition/IHE.PMIR.Feed.Audit",
        event_type: PATIENT_RECORD,
        subtypes: vec![ITI_93],
        action: "E",
        client: SOURCE_ROLE,
        server: DESTINATION_ROLE,
    }
}

/// What the Subscription Create audit profile fixes (BALP Create).
#[must_use]
pub fn subscription_create() -> EventKind {
    EventKind {
        profile: "https://profiles.ihe.net/ITI/PMIR/StructureDefinition/IHE.PMIR.Audit.Subscription.Create",
        event_type: REST,
        subtypes: vec![CREATE, ITI_94],
        action: "C",
        client: SOURCE_ROLE,
        server: DESTINATION_ROLE,
    }
}

/// What the Subscription Read audit profile fixes (BALP Read, whose data
/// flows from the server to the client).
#[must_use]
pub fn subscription_read() -> EventKind {
    EventKind {
        profile: "https://profiles.ihe.net/ITI/PMIR/StructureDefinition/IHE.PMIR.Audit.Subscription.Read",
        event_type: REST,
        subtypes: vec![ITI_94, READ],
        action: "R",
        client: DESTINATION_ROLE,
        server: SOURCE_ROLE,
    }
}

/// What the Subscription Delete audit profile fixes (BALP Delete).
#[must_use]
pub fn subscription_delete() -> EventKind {
    EventKind {
        profile: "https://profiles.ihe.net/ITI/PMIR/StructureDefinition/IHE.PMIR.Audit.Subscription.Delete",
        event_type: REST,
        subtypes: vec![DELETE, ITI_94],
        action: "D",
        client: APPLICATION,
        server: CUSTODIAN,
    }
}

/// What the BALP Query pattern fixes, for the search of subscriptions.
///
/// The search by channel endpoint is no ITI-94 interaction, and BALP's
/// basic patterns are for an event no more specific audit event defines.
#[must_use]
pub fn subscription_search() -> EventKind {
    EventKind {
        profile: "https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.Query",
        event_type: REST,
        subtypes: vec![SEARCH],
        action: "E",
        client: SOURCE_ROLE,
        server: DESTINATION_ROLE,
    }
}

/// How an ITI-94 exchange that ended in `result` is recorded.
fn outcome<T>(result: &Result<T, SubscribeError>) -> Outcome {
    match result {
        Ok(_) => Outcome::Success,
        Err(error) if error.answered() => Outcome::MinorFailure,
        Err(_) => Outcome::SeriousFailure,
    }
}

/// The Registry as an agent: named by its FHIR base, the directory its
/// `[base]/Subscription` URL `subscriptions` is in.
fn registry(subscriptions: &Url) -> Peer {
    Peer::server(
        &subscriptions
            .join(".")
            .unwrap_or_else(|_| subscriptions.clone()),
    )
}

/// The `Subscription/<id>` reference of a subscription at `location`.
pub(super) fn subscription_reference(location: &Url) -> Option<String> {
    location
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|id| !id.is_empty())
        .map(|id| format!("Subscription/{id}"))
}

/// The audit record of one ITI-94 interaction of `kind` with the Registry
/// whose `[base]/Subscription` URL is `subscriptions`, on the subscription
/// `reference` names, ending in `result`.
pub(super) fn subscription<T>(
    kind: EventKind,
    subscriptions: &Url,
    reference: Option<String>,
    criteria: Option<String>,
    result: &Result<T, SubscribeError>,
) -> Exchange {
    Exchange {
        kind,
        recorded: Timestamp::now(),
        outcome: outcome(result),
        outcome_description: None,
        direction: Direction::Sent {
            server: registry(subscriptions),
        },
        on_behalf: OnBehalfOf::System,
        entities: vec![Entity::Resource {
            reference,
            resource_type: "Subscription",
            kind: SYSTEM_OBJECT,
            role: Some(DOMAIN_RESOURCE),
            name: None,
            query: criteria,
        }],
        security: Vec::new(),
    }
}

/// The audit record of the search `request` of the Registry whose
/// `[base]/Subscription` URL is `subscriptions`.
pub(super) fn search<T>(
    subscriptions: &Url,
    request: &Url,
    result: &Result<T, SubscribeError>,
) -> Exchange {
    Exchange {
        kind: subscription_search(),
        recorded: Timestamp::now(),
        outcome: outcome(result),
        outcome_description: None,
        direction: Direction::Sent {
            server: registry(subscriptions),
        },
        on_behalf: OnBehalfOf::System,
        entities: vec![Entity::Query(request_text(request))],
        security: Vec::new(),
    }
}

/// Returns the audit record of one ITI-93 message a Consumer received.
///
/// The message arrived at the channel `endpoint` from the Registry at
/// `registry`: `feed` as read, or `None` for a message refused before it
/// could be read, whose patients cannot be named.
///
/// A message read is recorded as a success, a refused one as a minor
/// failure.
#[must_use]
pub fn received(registry: &Url, endpoint: &Url, feed: Option<&Feed>) -> Exchange {
    let mut entities = Vec::new();
    if let Some(feed) = feed {
        for event in feed.events() {
            match event {
                Event::Created(identity) | Event::Updated(identity) | Event::Deleted(identity) => {
                    entities.extend(patient(identity));
                }
                Event::Merged {
                    subsumed,
                    surviving,
                } => {
                    entities.extend(patient(subsumed));
                    entities.push(Entity::PatientReference(surviving.clone()));
                }
            }
        }
        entities.push(Entity::Resource {
            reference: Some(format!("MessageHeader/{}", feed.message_id())),
            resource_type: "MessageHeader",
            kind: MESSAGE_HEADER,
            role: None,
            name: Some(super::message::FEED_EVENT),
            query: None,
        });
    }
    Exchange {
        kind: feed_kind(),
        recorded: Timestamp::now(),
        outcome: if feed.is_some() {
            Outcome::Success
        } else {
            Outcome::MinorFailure
        },
        outcome_description: None,
        direction: Direction::Received {
            client: Peer::server(registry),
            endpoint: NetworkAddress::uri(endpoint),
        },
        on_behalf: OnBehalfOf::System,
        entities,
        security: Vec::new(),
    }
}

/// The patient entity of `identity`: its `Patient/<id>` reference, or its
/// first business identifier with a system when the entry carries no id.
fn patient(identity: &PatientIdentity) -> Option<Entity> {
    if let Some(id) = identity.id() {
        return Some(Entity::PatientReference(SecretString::from(format!(
            "Patient/{}",
            id.expose_secret()
        ))));
    }
    identity.identifiers().iter().find_map(|identifier| {
        identifier.system().map(|system| Entity::Patient {
            system: Some(system.to_owned()),
            namespace: None,
            value: identifier.value().clone(),
        })
    })
}
