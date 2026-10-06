// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! How an [`Exchange`] is written as its FHIR R4 `AuditEvent`: its agents,
//! the user an exchange was made for, and its entities.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use fhir_types::r4::audit_event::{
    AuditEvent, AuditEventAgent, AuditEventAgentNetwork, AuditEventEntity, AuditEventEntityDetail,
    AuditEventEntityDetailValue, AuditEventSource,
};
use fhir_types::r4::codeable_concept::CodeableConcept;
use fhir_types::r4::coding::Coding;
use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::meta::Meta;
use fhir_types::r4::primitives;
use fhir_types::r4::reference::Reference;
use secrecy::{ExposeSecret, SecretSlice};

use super::{
    APPLICATION, AuditRecord, Coded, Described, Direction, Entity, Exchange, INFORMATION_RECIPIENT,
    NetworkAddress, Observer, PATIENT_ROLE, PERSON, Peer, QUERY_ROLE, RecordError, SYSTEM_OBJECT,
    What,
};
use crate::user::{PurposeOfUse, User};

impl Exchange {
    /// The `AuditEvent` of this exchange as `observer` records it.
    ///
    /// # Errors
    ///
    /// A [`RecordError`] when the resource cannot be written as JSON, which
    /// writing to memory does not do.
    pub fn audit_event(&self, observer: &Observer) -> Result<AuditRecord, RecordError> {
        let own = |kind: Coded, network: NetworkAddress| AuditEventAgent {
            r#type: Some(concept(kind)),
            who: Some(display(&observer.source_id)),
            requestor: primitives::Boolean::from(false),
            network: Some(network_of(&network)),
            ..AuditEventAgent::default()
        };
        let other = |kind: Coded, peer: &Peer| AuditEventAgent {
            r#type: Some(concept(kind)),
            who: Some(display(&peer.who)),
            requestor: primitives::Boolean::from(false),
            network: Some(network_of(&peer.network)),
            ..AuditEventAgent::default()
        };
        // NOTE: BALP's client examples (AuditEvent-ex-auditBasicQueryGetClient and
        // the others) set requestor false on both system agents; only a user is one.
        let mut agent = match &self.direction {
            Direction::Sent { server } => vec![
                own(self.kind.client, observer.host.clone()),
                other(self.kind.server, server),
            ],
            Direction::Received { client, endpoint } => vec![
                other(self.kind.client, client),
                own(self.kind.server, NetworkAddress::uri(endpoint)),
            ],
        };
        if let Some(user) = self.on_behalf.user() {
            let [person, application] = user_agents(user);
            agent.push(person);
            // NOTE: BALP 1.1.4 IHE.BasicAudit.Delete admits one 110150 agent, its client, which
            // then names the user's application, so a second one is not added.
            if self.kind.client != APPLICATION {
                agent.push(application);
            }
            agent.extend(organisation_agent(user));
        }
        let event = AuditEvent {
            meta: (!self.kind.profile.is_empty()).then(|| Meta {
                profile: vec![primitives::Canonical::from(self.kind.profile)],
                ..Meta::default()
            }),
            r#type: self.kind.event_type.coding(),
            subtype: self
                .kind
                .subtypes
                .iter()
                .map(|code| code.coding())
                .collect(),
            action: Some(primitives::Code::from(self.kind.action)),
            recorded: primitives::Instant::from(self.recorded.to_string()),
            outcome: Some(primitives::Code::from(self.outcome.code())),
            agent,
            source: AuditEventSource {
                site: observer.site.as_deref().map(primitives::String::from),
                observer: display(&observer.source_id),
                ..AuditEventSource::default()
            },
            entity: self.entities.iter().map(entity).collect(),
            ..AuditEvent::default()
        };
        serde_json::to_vec(&event)
            .map(|json| AuditRecord(SecretSlice::from(json)))
            .map_err(RecordError)
    }
}

/// The two agents that name `user` (BALP 1.1.4 §3:5.7.5.4): the
/// `agent:user` slice with the token's `iss` and `sub` as `who.identifier`,
/// as `requestor`, with its purposes of use, and the Application agent with
/// the token's `client_id` as `who.identifier.value`.
fn user_agents(user: &User) -> [AuditEventAgent; 2] {
    let identified = |system: Option<&str>, value: &str| Reference {
        identifier: Some(Box::new(Identifier {
            system: system.map(primitives::Uri::from),
            value: Some(primitives::String::from(value)),
            ..Identifier::default()
        })),
        ..Reference::default()
    };
    // NOTE: BALP 1.1.4 agent:user fixes requestor true and allows no network; the
    // client's agent follows ex-auditBasicReadOServer, requestor false.
    let person = AuditEventAgent {
        r#type: Some(concept(INFORMATION_RECIPIENT)),
        who: Some(identified(Some(user.issuer()), user.subject())),
        alt_id: user.alt_id().map(primitives::String::from),
        requestor: primitives::Boolean::from(true),
        purpose_of_use: user.purposes().iter().map(purpose).collect(),
        ..AuditEventAgent::default()
    };
    let application = AuditEventAgent {
        r#type: Some(concept(APPLICATION)),
        who: Some(identified(None, user.client_id())),
        requestor: primitives::Boolean::from(false),
        ..AuditEventAgent::default()
    };
    [person, application]
}

/// The agent of the organisation `user` acts for, when one is named: `who`
/// an `Organization` by its identifier, with no `type`, so it fills no slice
/// a BALP pattern bounds, and not the `requestor`.
fn organisation_agent(user: &User) -> Option<AuditEventAgent> {
    let organisation = user.organisation()?;
    Some(AuditEventAgent {
        who: Some(Reference {
            r#type: Some(primitives::Uri::from("Organization")),
            identifier: Some(Box::new(Identifier {
                value: Some(primitives::String::from(organisation)),
                ..Identifier::default()
            })),
            ..Reference::default()
        }),
        requestor: primitives::Boolean::from(false),
        ..AuditEventAgent::default()
    })
}

/// A purpose of use as `agent.purposeOfUse` codes it.
fn purpose(purpose: &PurposeOfUse) -> CodeableConcept {
    CodeableConcept {
        coding: vec![Coding {
            system: purpose.system.as_deref().map(primitives::Uri::from),
            code: Some(primitives::Code::from(purpose.code.as_str())),
            ..Coding::default()
        }],
        ..CodeableConcept::default()
    }
}

fn concept(code: Coded) -> CodeableConcept {
    CodeableConcept {
        coding: vec![code.coding()],
        ..CodeableConcept::default()
    }
}

fn display(text: &str) -> Reference {
    Reference {
        display: Some(primitives::String::from(text)),
        ..Reference::default()
    }
}

fn network_of(address: &NetworkAddress) -> AuditEventAgentNetwork {
    AuditEventAgentNetwork {
        address: Some(primitives::String::from(address.address())),
        r#type: Some(primitives::Code::from(address.type_code())),
        ..AuditEventAgentNetwork::default()
    }
}

fn entity(entity: &Entity) -> AuditEventEntity {
    match entity {
        Entity::Query(request) => AuditEventEntity {
            r#type: Some(SYSTEM_OBJECT.coding()),
            role: Some(QUERY_ROLE.coding()),
            query: Some(primitives::Base64Binary::from(
                STANDARD.encode(request.expose_secret()),
            )),
            ..AuditEventEntity::default()
        },
        Entity::Patient { system, value } => AuditEventEntity {
            what: Some(Reference {
                identifier: Some(Box::new(Identifier {
                    system: Some(primitives::Uri::from(system.as_str())),
                    value: Some(primitives::String::from(value.expose_secret())),
                    ..Identifier::default()
                })),
                ..Reference::default()
            }),
            r#type: Some(PERSON.coding()),
            role: Some(PATIENT_ROLE.coding()),
            ..AuditEventEntity::default()
        },
        Entity::PatientReference(reference) => AuditEventEntity {
            what: Some(Reference {
                reference: Some(primitives::String::from(reference.expose_secret())),
                ..Reference::default()
            }),
            r#type: Some(PERSON.coding()),
            role: Some(PATIENT_ROLE.coding()),
            ..AuditEventEntity::default()
        },
        Entity::Resource {
            reference,
            resource_type,
            kind,
            role,
            name,
            query,
        } => AuditEventEntity {
            what: Some(Reference {
                reference: reference.as_deref().map(primitives::String::from),
                r#type: Some(primitives::Uri::from(*resource_type)),
                ..Reference::default()
            }),
            r#type: Some(kind.coding()),
            role: role.map(Coded::coding),
            name: name.map(primitives::String::from),
            query: query
                .as_deref()
                .map(|text| primitives::Base64Binary::from(STANDARD.encode(text))),
            ..AuditEventEntity::default()
        },
        Entity::Described(described) => described_entity(described),
    }
}

/// The `entity` a [`Described`] entity is written as.
fn described_entity(described: &Described) -> AuditEventEntity {
    let what = described.what.as_ref().map(|what| match what {
        What::Reference {
            reference,
            resource_type,
        } => Reference {
            reference: Some(primitives::String::from(reference.as_str())),
            r#type: resource_type.as_deref().map(primitives::Uri::from),
            ..Reference::default()
        },
        What::Identifier { system, value } => Reference {
            identifier: Some(Box::new(Identifier {
                system: system.as_deref().map(primitives::Uri::from),
                value: Some(primitives::String::from(value.as_str())),
                ..Identifier::default()
            })),
            ..Reference::default()
        },
    });
    AuditEventEntity {
        what,
        r#type: Some(described.kind.coding()),
        role: described.role.map(Coded::coding),
        name: described.name.as_deref().map(primitives::String::from),
        description: described
            .description
            .as_deref()
            .map(primitives::String::from),
        detail: described
            .details
            .iter()
            .map(|detail| AuditEventEntityDetail {
                id: None,
                extension: Vec::new(),
                modifier_extension: Vec::new(),
                r#type: primitives::String::from(detail.kind.as_str()),
                value: AuditEventEntityDetailValue::String(primitives::String::from(
                    detail.value.as_str(),
                )),
            })
            .collect(),
        ..AuditEventEntity::default()
    }
}
