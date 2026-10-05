// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! BALP, the IHE Basic Audit Log Patterns 1.1.4 (feature `balp`): the FHIR R4
//! `AuditEvent` an IHE FHIR transaction is audited as.
//!
//! PIXm, mCSD, PDQm and PMIR each define the audit record of their
//! transactions as an `AuditEvent` profile built on a BALP pattern (PIXm
//! §2:3.83.5.1, mCSD §2:3.90.5.1 and §2:3.91.5.1, PDQm §2:3.78.5.1, PMIR
//! §2:3.93.5.1 and §2:3.94.5.1). A client of this crate that is
//! [audited](AuditRecorder) hands its recorder one [`Exchange`] per
//! transaction: what the transaction's audit profile fixes ([`EventKind`]),
//! the outcome, the other party, whom the exchange was made for
//! ([`OnBehalfOf`]), and the entities the event concerned.
//! [`Exchange::audit_event`] writes the `AuditEvent` with the
//! [`Observer`] that records it, the system the client runs on, which the
//! client does not know.
//!
//! BALP has its Audit Creator send the record over the ATNA ATX: FHIR Feed
//! Option (BALP §1:52.1.1.1; ITI TF-1 §9.2.7.1 as the `RESTful` ATNA supplement
//! adds it), the `create` of ITI-20 §3.20.4.2 that
//! [`crate::atna::feed`] makes.
//!
//! An `Exchange` may name a patient, in a [`Entity::Patient`] identifier or
//! inside the search request of an [`Entity::Query`], as the audit profiles
//! require: every such value is a [`SecretString`], `Debug` redacts it, and
//! the written record is an [`AuditRecord`], whose bytes `Debug` never shows.
//! A recorder that logs an exchange must leave them out. An `Exchange` made
//! for a user names them too, as the audit record names its requestor (PIXm
//! §2:3.83.5.2.1): `Debug` shows none of the user's values, and a recorder
//! sends them to its Audit Record Repository alone.

use std::fmt;
use std::net::IpAddr;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use fhir_types::r4::audit_event::{
    AuditEvent, AuditEventAgent, AuditEventAgentNetwork, AuditEventEntity, AuditEventSource,
};
use fhir_types::r4::codeable_concept::CodeableConcept;
use fhir_types::r4::coding::Coding;
use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::meta::Meta;
use fhir_types::r4::primitives;
use fhir_types::r4::reference::Reference;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretSlice, SecretString};
use url::Url;

use crate::redact::{REDACTED, RedactedUrl};
use crate::user::{OnBehalfOf, PurposeOfUse, User};

/// A coding a profile fixes: its `system`, `code` and `display`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coded {
    /// `system`.
    pub system: &'static str,
    /// `code`.
    pub code: &'static str,
    /// `display`.
    pub display: &'static str,
}

impl Coded {
    fn coding(self) -> Coding {
        Coding {
            system: Some(primitives::Uri::from(self.system)),
            code: Some(primitives::Code::from(self.code)),
            display: Some(primitives::String::from(self.display)),
            ..Coding::default()
        }
    }
}

/// The code system of IHE transaction codes, the `urn:ihe:event-type-code`
/// subtype every IHE audit profile adds.
pub const IHE_TRANSACTIONS: &str = "urn:ihe:event-type-code";

/// `AuditEvent.type` of every `RESTful` pattern: `rest`
/// (`IHE.BasicAudit.Query`, `.Read`, `.Create`, `.Delete`).
pub const REST: Coded = Coded {
    system: "http://terminology.hl7.org/CodeSystem/audit-event-type",
    code: "rest",
    display: "Restful Operation",
};

/// The agent type of the side the request flows from: DICOM `110153`.
pub const SOURCE_ROLE: Coded = Coded {
    system: "http://dicom.nema.org/resources/ontology/DCM",
    code: "110153",
    display: "Source Role ID",
};

/// The agent type of the side the request flows to: DICOM `110152`.
pub const DESTINATION_ROLE: Coded = Coded {
    system: "http://dicom.nema.org/resources/ontology/DCM",
    code: "110152",
    display: "Destination Role ID",
};

/// The client's agent type of the Delete pattern: DICOM `110150`
/// (`IHE.BasicAudit.Delete`, `agent:client.type`).
pub const APPLICATION: Coded = Coded {
    system: "http://dicom.nema.org/resources/ontology/DCM",
    code: "110150",
    display: "Application",
};

/// The agent type of the user an exchange is made for: `IRCP`, as every
/// BALP pattern's `agent:user` slice fixes it.
pub const INFORMATION_RECIPIENT: Coded = Coded {
    system: "http://terminology.hl7.org/CodeSystem/v3-ParticipationType",
    code: "IRCP",
    display: "information recipient",
};

/// The server's agent type of the Delete pattern: `custodian`
/// (`IHE.BasicAudit.Delete`, `agent:server.type`).
pub const CUSTODIAN: Coded = Coded {
    system: "http://terminology.hl7.org/CodeSystem/provenance-participant-type",
    code: "custodian",
    display: "Custodian",
};

/// The `restful-interaction` subtype `search` (`IHE.BasicAudit.Query`,
/// `subtype:anySearch`).
pub const SEARCH: Coded = Coded {
    system: "http://hl7.org/fhir/restful-interaction",
    code: "search",
    display: "search",
};

/// The `restful-interaction` subtype `read` (`IHE.BasicAudit.Read`).
pub const READ: Coded = Coded {
    system: "http://hl7.org/fhir/restful-interaction",
    code: "read",
    display: "read",
};

/// The `restful-interaction` subtype `create` (`IHE.BasicAudit.Create`,
/// `subtype:anyCreate`).
pub const CREATE: Coded = Coded {
    system: "http://hl7.org/fhir/restful-interaction",
    code: "create",
    display: "create",
};

/// The `restful-interaction` subtype `delete` (`IHE.BasicAudit.Delete`,
/// `subtype:anyDelete`).
pub const DELETE: Coded = Coded {
    system: "http://hl7.org/fhir/restful-interaction",
    code: "delete",
    display: "delete",
};

/// The `restful-interaction` subtype `history-type` (the mCSD Updates audit
/// profile, `subtype:anyHistoryT`).
pub const HISTORY_TYPE: Coded = Coded {
    system: "http://hl7.org/fhir/restful-interaction",
    code: "history-type",
    display: "history-type",
};

/// The entity type of a system object: `2` (`audit-entity-type`).
pub const SYSTEM_OBJECT: Coded = Coded {
    system: "http://terminology.hl7.org/CodeSystem/audit-entity-type",
    code: "2",
    display: "System Object",
};

/// The entity type of a person: `1` (`audit-entity-type`).
pub const PERSON: Coded = Coded {
    system: "http://terminology.hl7.org/CodeSystem/audit-entity-type",
    code: "1",
    display: "Person",
};

/// The entity role of a search: `24` (`object-role`).
pub const QUERY_ROLE: Coded = Coded {
    system: "http://terminology.hl7.org/CodeSystem/object-role",
    code: "24",
    display: "Query",
};

/// The entity role of a resource: `4` (`object-role`), as the BALP and
/// PMIR examples give a resource acted on.
pub const DOMAIN_RESOURCE: Coded = Coded {
    system: "http://terminology.hl7.org/CodeSystem/object-role",
    code: "4",
    display: "Domain Resource",
};

/// The entity role of a patient: `1` (`object-role`).
pub const PATIENT_ROLE: Coded = Coded {
    system: "http://terminology.hl7.org/CodeSystem/object-role",
    code: "1",
    display: "Patient",
};

/// What a transaction's audit profile fixes of its records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventKind {
    /// The audit profile's canonical URL, written to `meta.profile`.
    pub profile: &'static str,
    /// `type`.
    pub event_type: Coded,
    /// `subtype`, in order.
    pub subtypes: &'static [Coded],
    /// `action`: `C`, `R`, `U`, `D` or `E`.
    pub action: &'static str,
    /// The type of the agent the request flows from (`agent:client` of a
    /// BALP pattern).
    pub client: Coded,
    /// The type of the agent the request flows to (`agent:server`).
    pub server: Coded,
}

/// How an exchange ended, as `AuditEvent.outcome` codes it (FHIR R4
/// `audit-event-outcome`).
///
/// The codes leave the meaning of a failure to the recording application;
/// an answer and its absence are told apart (no specification governs the
/// choice: our own design, the one the ITI-55 audit message makes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// `0`, success: the answer was read as the transaction's answer.
    Success,
    /// `4`, minor failure: the other party answered with a failure, or with
    /// an answer that does not hold to the transaction.
    MinorFailure,
    /// `8`, serious failure: no answer arrived.
    SeriousFailure,
}

impl Outcome {
    /// The `outcome` code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Success => "0",
            Self::MinorFailure => "4",
            Self::SeriousFailure => "8",
        }
    }
}

/// An agent's `network`: its address and the `network.type` code
/// (`network-type`: `1` a machine name, `2` an IP address, `5` a URI).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NetworkAddress {
    /// A machine (DNS) name, type `1`.
    MachineName(String),
    /// An IP address, type `2`.
    Ip(IpAddr),
    /// A URI, type `5`, without its userinfo, query or fragment.
    Uri(Url),
}

impl NetworkAddress {
    /// The address of `host`: an IP address when it reads as one, and a
    /// machine name otherwise.
    #[must_use]
    pub fn host(host: &str) -> Self {
        host.parse::<IpAddr>()
            .map_or_else(|_| Self::MachineName(host.to_owned()), Self::Ip)
    }

    /// The address of the URI `url`, with its userinfo, query and fragment
    /// left out: a FHIR base or an endpoint names no patient.
    #[must_use]
    pub fn uri(url: &Url) -> Self {
        Self::Uri(bare(url))
    }

    /// `network.type`.
    #[must_use]
    pub fn type_code(&self) -> &'static str {
        match self {
            Self::MachineName(_) => "1",
            Self::Ip(_) => "2",
            Self::Uri(_) => "5",
        }
    }

    /// `network.address`.
    #[must_use]
    pub fn address(&self) -> String {
        match self {
            Self::MachineName(name) => name.clone(),
            Self::Ip(address) => address.to_string(),
            Self::Uri(url) => url.to_string(),
        }
    }
}

/// The request `url` as an audit record writes it in a query entity: its
/// query kept, since that is the search, and its userinfo left out.
#[must_use]
pub fn request_text(url: &Url) -> SecretString {
    let mut shown = url.clone();
    // NOTE: url::Url::set_username and set_password fail only for a URL that
    // cannot carry userinfo, which then has none to remove.
    let _cleared: Result<(), ()> = shown.set_username("");
    let _cleared: Result<(), ()> = shown.set_password(None);
    shown.set_fragment(None);
    SecretString::from(String::from(shown))
}

/// `url` without its userinfo, query and fragment.
fn bare(url: &Url) -> Url {
    let mut shown = url.clone();
    // NOTE: url::Url::set_username and set_password fail only for a URL that
    // cannot carry userinfo, which then has none to remove.
    let _cleared: Result<(), ()> = shown.set_username("");
    let _cleared: Result<(), ()> = shown.set_password(None);
    shown.set_query(None);
    shown.set_fragment(None);
    shown
}

/// A party to an exchange other than the recording system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    /// `agent.who.display`: how the party is known.
    pub who: String,
    /// `agent.network`.
    pub network: NetworkAddress,
}

impl Peer {
    /// The FHIR server at `base`: named and addressed by the URL, without
    /// its userinfo, query or fragment, as the profiles' examples name a
    /// server.
    #[must_use]
    pub fn server(base: &Url) -> Self {
        let shown = bare(base);
        Self {
            who: shown.to_string(),
            network: NetworkAddress::Uri(shown),
        }
    }
}

/// The recording system's part in the exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Direction {
    /// The recording system sent the request to `server`: it is the agent
    /// the request flows from.
    Sent {
        /// The agent the request flows to.
        server: Peer,
    },
    /// `client` sent the request to the recording system, at `endpoint`: the
    /// recording system is the agent the request flows to.
    Received {
        /// The agent the request flows from.
        client: Peer,
        /// The recording system's endpoint the request arrived at.
        endpoint: Url,
    },
}

/// An entity an exchange concerned.
#[derive(Clone)]
#[non_exhaustive]
pub enum Entity {
    /// The request as sent (`entity:query`, BALP Query): type `2`, role
    /// `24`, the request written base64-encoded to `query`. A search request
    /// may name a patient.
    Query(SecretString),
    /// A patient, by an identifier (`entity:patient`): type `1`, role `1`,
    /// the identifier in `what.identifier`.
    Patient {
        /// `what.identifier.system`.
        system: String,
        /// `what.identifier.value`.
        value: SecretString,
    },
    /// A patient, by a literal reference such as `Patient/<id>`
    /// (`entity:patient`): type `1`, role `1`, the reference in
    /// `what.reference`.
    PatientReference(SecretString),
    /// A resource the exchange acted on (`entity:data` of BALP Read, Create
    /// and Delete, or a profile's own slice): `what`, `type`, `role` when
    /// the profile fixes one, `name` and `query`.
    Resource {
        /// `what.reference`, a literal reference that names no patient, or
        /// `None` when the resource has no id to name, as a create that
        /// failed has none.
        reference: Option<String>,
        /// `what.type`, the resource type.
        resource_type: &'static str,
        /// `type`.
        kind: Coded,
        /// `role`.
        role: Option<Coded>,
        /// `name`.
        name: Option<&'static str>,
        /// `query`, written base64-encoded: text that names no patient,
        /// such as a subscription's criteria.
        query: Option<String>,
    },
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query(_) => f.debug_tuple("Query").field(&REDACTED).finish(),
            Self::PatientReference(_) => {
                f.debug_tuple("PatientReference").field(&REDACTED).finish()
            }
            Self::Patient { system, .. } => f
                .debug_struct("Patient")
                .field("system", system)
                .field("value", &REDACTED)
                .finish(),
            Self::Resource {
                reference,
                resource_type,
                kind,
                role,
                name,
                query,
            } => f
                .debug_struct("Resource")
                .field("reference", reference)
                .field("resource_type", resource_type)
                .field("kind", kind)
                .field("role", role)
                .field("name", name)
                .field("query", query)
                .finish(),
        }
    }
}

/// One exchange as the transaction knows it; the [`Observer`] that records
/// it is added by [`Exchange::audit_event`].
///
/// An exchange made for a [`User`] names them in two agents, as BALP 1.1.4
/// §3:5.7.5.4 maps an OAuth token's fields: the `agent:user` slice every BALP
/// pattern has (`0..1`) with the token's `iss`, `sub` and purposes of use,
/// and an Application agent with its `client_id`. One the system makes on
/// its own behalf names neither. The Delete pattern admits one `110150`
/// agent, its own `agent:client`, so a Delete exchange is the system's.
#[derive(Clone)]
pub struct Exchange {
    /// What the transaction's audit profile fixes.
    pub kind: EventKind,
    /// `recorded`: when the exchange ended.
    pub recorded: Timestamp,
    /// `outcome`.
    pub outcome: Outcome,
    /// The recording system's part, and the other party.
    pub direction: Direction,
    /// Whom the exchange was made for.
    pub on_behalf: OnBehalfOf,
    /// `entity`, in order.
    pub entities: Vec<Entity>,
}

impl fmt::Debug for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let direction = match &self.direction {
            Direction::Sent { server } => format!("sent to {:?}", RedactedUrl(&server.who)),
            Direction::Received { endpoint, .. } => {
                format!("received at {:?}", RedactedUrl(endpoint.as_str()))
            }
        };
        f.debug_struct("Exchange")
            .field("profile", &self.kind.profile)
            .field("recorded", &self.recorded)
            .field("outcome", &self.outcome)
            .field("direction", &direction)
            .field("on_behalf", &self.on_behalf)
            .field("entities", &self.entities)
            .finish()
    }
}

/// The system that records an exchange: `source` and its own agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observer {
    /// `source.observer.display`, and the `who.display` of the recording
    /// system's agent.
    pub source_id: String,
    /// `source.site`, when the deployment names one.
    pub site: Option<String>,
    /// The recording system's `network` when it sends a request.
    pub host: NetworkAddress,
}

/// One written `AuditEvent`, FHIR JSON (ITI TF-2 Appendix Z.6), held as a
/// secret: it may name a patient.
pub struct AuditRecord(SecretSlice<u8>);

impl AuditRecord {
    /// The JSON bytes.
    #[must_use]
    pub fn into_bytes(self) -> SecretSlice<u8> {
        self.0
    }
}

impl fmt::Debug for AuditRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AuditRecord").field(&REDACTED).finish()
    }
}

/// Why a record could not be written.
#[derive(Debug, thiserror::Error)]
#[error("the AuditEvent could not be written")]
pub struct RecordError(#[source] serde_json::Error);

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
            agent.extend(user_agents(user));
        }
        let event = AuditEvent {
            meta: Some(Meta {
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
    }
}

/// Where an audited client's exchanges go: a deployment's route to its ATNA
/// Audit Record Repository.
///
/// [`AuditRecorder::record`] is awaited once per transaction, before its
/// answer is returned: a recorder that sends over the network stores the
/// record first and delivers it from there, as ITI-20 has a sender do (ITI
/// TF-2 §3.20.4.1.1), and returns once it is stored.
///
/// The audit record is part of the transaction: an exchange the recorder
/// cannot accept fails the transaction, and its answer is never used.
#[async_trait::async_trait]
pub trait AuditRecorder: Send + Sync {
    /// Records `exchange`.
    ///
    /// # Errors
    ///
    /// An [`AuditError`] when the recorder cannot accept the exchange.
    async fn record(&self, exchange: Exchange) -> Result<(), AuditError>;
}

/// Why an audit recorder could not accept an exchange.
#[derive(Debug, thiserror::Error)]
#[error("the audit recorder could not accept the audit record")]
pub struct AuditError(#[source] pub Box<dyn std::error::Error + Send + Sync>);
