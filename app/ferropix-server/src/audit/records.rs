// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `AuditEvent` each transaction writes as the Manager or the Supplier,
//! and each steward action, as BALP 1.1.4 and the profiles fix them.
//!
//! - ITI-83: the PIXm Query Manager audit profile, on BALP Patient Query
//!   (PIXm 3.1.0 §2:3.83.5.1.2), with the source identifier as the patient.
//! - ITI-104: the PIXm Feed Create, Update and Delete Manager audit
//!   profiles, on BALP Patient Create, Update and Delete (§2:3.104.5.1.2),
//!   with the fed identifier as the patient and the person as the data.
//! - ITI-78 and ITI-119: the PDQm Query and Match Supplier audit profiles,
//!   on BALP Query (PDQm 3.2.0 §2:3.78.5.1.2, §2:3.119.5.1.2), one record
//!   per person found and one with no patient when none was.
//! - ITI-8, ITI-9 and ITI-21 over the HL7 v2 face: the Manager's and the
//!   Supplier's audit messages of ITI TF-2 §3.8.5.1.2, §3.8.5.2.2,
//!   §3.9.5.1.2 and §3.21.5.1.2, written as `AuditEvent`s: the DICOM event
//!   id as `type`, the transaction as `subtype`, the sender as the source
//!   agent and the message as the query entity of a query.
//! - ITI-55 as the XCPD Responding Gateway: the audit message of ITI TF-2
//!   §3.55.5.1.2 written as an `AuditEvent` the same way, with the
//!   `QueryByParameter` as the query entity, the initiating community as its
//!   `ihe:homeCommunityID` detail and one patient per patient answered.
//! - ITI-93 as the PMIR Registry: the PMIR Feed audit profile (PMIR 1.6.0
//!   §2:3.93.5.1), one record per message, with each Patient the message
//!   names and its `MessageHeader`.
//! - A pseudonym minted on an ITI-83 ask, on BALP Create, naming the
//!   person and the pseudonymisation domain, never the value.
//! - A steward action, in [`operator`]: a `RESTful` activity on the person (BALP 1.1.4
//!   §3:5.7.3), a read for a view and an update for a decision; a steward's
//!   search, a BALP Patient Query record per person found or a Query record
//!   when none was; and an export of the trail, an Export event.
//!
//! A search's raw request is written base64 to `entity.query` (BALP 1.1.4
//! §1:52.4.1.4) and the patient is the subject entity (§1:52.4.1.6), which
//! is why the audit record, and only the audit record, carries an
//! identifier.

use std::net::IpAddr;

use ferrohealth_smart::resource::{Access, Via};
use ihe_atna::balp::{
    APPLICATION, CREATE, CUSTODIAN, Coded, DCM, DELETE, DESTINATION_ROLE, DOMAIN_RESOURCE,
    Described, Detail, Direction, Entity, EventKind, Exchange, IHE_TRANSACTIONS, NetworkAddress,
    Outcome, Peer, READ, REQUEST_ID, REST, SEARCH, SOURCE_ROLE, SYSTEM_OBJECT, UPDATE, What,
    request_text,
};
use ihe_atna::user::{OnBehalfOf, User};
use secrecy::{ExposeSecret, SecretString};

pub mod operator;

/// The `ITI-83` subtype.
pub const ITI_83: Coded = Coded::fixed(
    IHE_TRANSACTIONS,
    "ITI-83",
    "Mobile Patient Identifier Cross-reference Query",
);

/// The `ITI-104` subtype.
pub const ITI_104: Coded = Coded::fixed(IHE_TRANSACTIONS, "ITI-104", "Patient Identity Feed FHIR");

/// The `ITI-78` subtype.
pub const ITI_78: Coded = Coded::fixed(
    IHE_TRANSACTIONS,
    "ITI-78",
    "Mobile Patient Demographics Query",
);

/// The `ITI-119` subtype.
pub const ITI_119: Coded = Coded::fixed(IHE_TRANSACTIONS, "ITI-119", "Patient Demographics Match");

/// The `ITI-8` subtype (ITI TF-2 §3.8.5.1.2: `EV("ITI-8", "IHE
/// Transactions", "Patient Identity Feed")`).
pub const ITI_8: Coded = Coded::fixed(IHE_TRANSACTIONS, "ITI-8", "Patient Identity Feed");

/// The `ITI-9` subtype (ITI TF-2 §3.9.5.1.2).
pub const ITI_9: Coded = Coded::fixed(IHE_TRANSACTIONS, "ITI-9", "PIX Query");

/// The `ITI-21` subtype (ITI TF-2 §3.21.5.1.2).
pub const ITI_21: Coded = Coded::fixed(IHE_TRANSACTIONS, "ITI-21", "Patient Demographics Query");

/// The `ITI-55` subtype (ITI TF-2 §3.55.5.1.2: `EV("ITI-55", "IHE
/// Transactions", "Cross Gateway Patient Discovery")`).
pub const ITI_55: Coded = Coded::fixed(
    IHE_TRANSACTIONS,
    "ITI-55",
    "Cross Gateway Patient Discovery",
);

/// The detail type of the community a query names (ITI TF-2 §3.55.5.1.2:
/// `ihe:homeCommunityID`, a literal string).
pub const HOME_COMMUNITY_DETAIL: &str = "ihe:homeCommunityID";

/// `EV(110110, DCM, "Patient Record")`, the event of a feed (ITI TF-2
/// §3.8.5.1.2).
pub const PATIENT_RECORD: Coded = Coded::fixed(DCM, "110110", "Patient Record");

/// `EV(110112, DCM, "Query")`, the event of a query (ITI TF-2 §3.9.5.1.2,
/// §3.21.5.1.2).
pub const QUERY: Coded = Coded::fixed(DCM, "110112", "Query");

/// Who called and where, as every record of one request names them.
#[derive(Debug, Clone)]
pub struct Call {
    /// The client: its client id or subject, at its network address.
    pub client: Peer,
    /// Whom the request was made for.
    pub on_behalf: OnBehalfOf,
    /// The FHIR base the request arrived at.
    pub endpoint: NetworkAddress,
    /// The request id, recorded as the transaction entity.
    pub request_id: Option<String>,
}

impl Call {
    /// Returns the call of `access` from `remote`, at the FHIR base
    /// `endpoint`, under `request_id`.
    ///
    /// A token whose subject is its own client is a backend service acting
    /// for itself, recorded with no user (BALP's `NoUser` examples); any
    /// other names its user from the token (BALP 1.1.4 §3:5.7.5.4). The
    /// client's address is its IP address, or the FHIR base's host when the
    /// listener did not see one (BALP 1.1.4 §1:52.4.1.5, best effort; our
    /// own design).
    #[must_use]
    pub fn of(
        access: &Access,
        remote: Option<IpAddr>,
        endpoint: &url::Url,
        request_id: Option<&str>,
    ) -> Self {
        let (who, on_behalf) = match access.caller() {
            None => (String::from("anonymous"), OnBehalfOf::System),
            Some(caller) => {
                let client = caller.client_id().unwrap_or(caller.subject()).to_owned();
                let on_behalf = match (caller.via(), caller.client_id()) {
                    (Via::Basic, _) => OnBehalfOf::User(User::local(caller.subject().to_owned())),
                    (Via::Bearer, Some(id)) if id == caller.subject() => OnBehalfOf::System,
                    (Via::Bearer, _) => OnBehalfOf::User(
                        User::local(caller.subject().to_owned())
                            .with_issuer(caller.issuer().map(str::to_owned))
                            .with_client_id(caller.client_id().map(str::to_owned)),
                    ),
                };
                (client, on_behalf)
            }
        };
        let network = remote.map_or_else(
            || NetworkAddress::host(endpoint.host_str().unwrap_or("unknown")),
            NetworkAddress::Ip,
        );
        Self {
            client: Peer { who, network },
            on_behalf,
            endpoint: NetworkAddress::uri(endpoint),
            request_id: request_id.map(str::to_owned),
        }
    }

    /// Returns the exchange of `kind` this call made, ended in `outcome`,
    /// concerning `entities`.
    fn exchange(&self, kind: EventKind, outcome: Outcome, mut entities: Vec<Entity>) -> Exchange {
        if let Some(id) = &self.request_id {
            entities.push(Entity::Described(Described {
                what: Some(What::Identifier {
                    system: None,
                    value: id.clone(),
                }),
                kind: REQUEST_ID,
                role: None,
                name: None,
                description: None,
                details: Vec::new(),
                security_labels: Vec::new(),
            }));
        }
        Exchange {
            kind,
            recorded: jiff::Timestamp::now(),
            outcome,
            outcome_description: None,
            direction: Direction::Received {
                client: self.client.clone(),
                endpoint: self.endpoint.clone(),
            },
            on_behalf: self.on_behalf.clone(),
            entities,
            security: Vec::new(),
        }
    }
}

/// A patient identifier as a request named it: its system and its value.
#[derive(Clone)]
pub struct Identifier {
    /// The identifier's system, the domain.
    pub system: String,
    /// The identifier's value, which names the patient.
    pub value: SecretString,
}

impl std::fmt::Debug for Identifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identifier")
            .field("system", &self.system)
            .finish_non_exhaustive()
    }
}

impl Identifier {
    /// Returns the patient entity of this identifier.
    fn patient(&self) -> Entity {
        Entity::Patient {
            system: Some(self.system.clone()),
            value: self.value.clone(),
            namespace: None,
        }
    }
}

/// The raw request of a search as BALP 1.1.4 §1:52.4.1.4 has it recorded:
/// the URL of a `GET`, or the request line, media type and body of a `POST`.
#[must_use]
pub fn raw_request(url: &url::Url, post: Option<(&str, &SecretString)>) -> SecretString {
    match post {
        None => request_text(url),
        Some((media_type, body)) => SecretString::from(format!(
            "POST {}\nContent-Type: {media_type}\n\n{}",
            request_text(url).expose_secret(),
            body.expose_secret()
        )),
    }
}

/// Returns the record of an ITI-83 query for `source`, its raw request
/// `query`, ended in `outcome` (PIXm 3.1.0 §2:3.83.5.1.2).
#[must_use]
pub fn pix_query(
    call: &Call,
    query: SecretString,
    source: &Identifier,
    outcome: Outcome,
) -> Exchange {
    call.exchange(
        EventKind {
            profile: "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Query.Audit.Manager",
            event_type: REST,
            subtypes: vec![ITI_83, SEARCH],
            action: "E",
            client: SOURCE_ROLE,
            server: DESTINATION_ROLE,
        },
        outcome,
        vec![Entity::Query(query), source.patient()],
    )
}

/// What an ITI-104 feed did to the cross-reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    /// The identifier was new: a record was created.
    Create,
    /// The identifier was known: its record was revised.
    Update,
    /// The identifier was removed (the Remove Patient Option).
    Delete,
}

/// Returns the record of an ITI-104 feed of `identifier` to the person
/// `person` (`Patient/{id}`), ended in `outcome` (PIXm 3.1.0
/// §2:3.104.5.1.2).
#[must_use]
pub fn feed(
    call: &Call,
    action: Feed,
    identifier: &Identifier,
    person: Option<&str>,
    outcome: Outcome,
) -> Exchange {
    let (profile, subtype, code, client, server) = match action {
        Feed::Create => (
            "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Feed.Create.Audit.Manager",
            CREATE,
            "C",
            SOURCE_ROLE,
            DESTINATION_ROLE,
        ),
        Feed::Update => (
            "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Feed.Update.Audit.Manager",
            UPDATE,
            "U",
            SOURCE_ROLE,
            DESTINATION_ROLE,
        ),
        Feed::Delete => (
            "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Feed.Delete.Audit.Manager",
            DELETE,
            "D",
            APPLICATION,
            CUSTODIAN,
        ),
    };
    call.exchange(
        EventKind {
            profile,
            event_type: REST,
            subtypes: vec![subtype, ITI_104],
            action: code,
            client,
            server,
        },
        outcome,
        vec![person_data(person), identifier.patient()],
    )
}

/// Returns the record of an ITI-8 message that did `action` to `identifier`.
///
/// The message ended in `outcome`; ITI TF-2 §3.8.5.1.2 governs an add or a
/// revision and §3.8.5.2.2 the two sides of a merge: `U` for the survivor,
/// `D` for the subsumed identifier.
#[must_use]
pub fn v2_feed(call: &Call, action: Feed, identifier: &Identifier, outcome: Outcome) -> Exchange {
    let code = match action {
        Feed::Create => "C",
        Feed::Update => "U",
        Feed::Delete => "D",
    };
    call.exchange(
        EventKind {
            profile: "",
            event_type: PATIENT_RECORD,
            subtypes: vec![ITI_8],
            action: code,
            client: SOURCE_ROLE,
            server: DESTINATION_ROLE,
        },
        outcome,
        vec![identifier.patient()],
    )
}

/// The query transaction of the HL7 v2 face a record is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V2Query {
    /// ITI-9, the PIX Query.
    Pix,
    /// ITI-21, the Patient Demographics Query.
    Pdq,
}

/// Returns the record of a v2 `query` that ended in `outcome`.
///
/// It carries the whole query message `message`, which the record writes
/// base64-encoded (ITI TF-2 §3.9.5.1.2: "The complete query message
/// (including MSH and QPD segments), base64 encoded"), and the patients the
/// query concerned.
///
/// MSH-10 is the request id the call carries, its transaction entity.
#[must_use]
pub fn v2_query(
    call: &Call,
    query: V2Query,
    message: SecretString,
    identifiers: &[Identifier],
    persons: &[String],
    outcome: Outcome,
) -> Exchange {
    let subtype = match query {
        V2Query::Pix => ITI_9,
        V2Query::Pdq => ITI_21,
    };
    let mut entities = vec![Entity::Query(message)];
    entities.extend(identifiers.iter().map(Identifier::patient));
    entities.extend(
        persons
            .iter()
            .map(|person| Entity::PatientReference(SecretString::from(person.clone()))),
    );
    call.exchange(
        EventKind {
            profile: "",
            event_type: QUERY,
            subtypes: vec![subtype],
            action: "E",
            client: SOURCE_ROLE,
            server: DESTINATION_ROLE,
        },
        outcome,
        entities,
    )
}

/// Returns the record of an ITI-55 query a Responding Gateway answered,
/// ended in `outcome` (ITI TF-2 §3.55.5.1.2).
///
/// It carries the query's `QueryByParameter` element `query`, which the
/// record writes base64-encoded, with the initiating `community` as its
/// `ihe:homeCommunityID` detail when the query named one, and `patients`,
/// the identifier of each patient whose demographics the answer returned.
/// The detail sits on a system object beside the query entity, which carries
/// no detail; no specification governs that place: our own design.
#[must_use]
pub fn xcpd_query(
    call: &Call,
    query: SecretString,
    community: Option<&str>,
    patients: &[Identifier],
    outcome: Outcome,
) -> Exchange {
    let mut entities = vec![Entity::Query(query)];
    if let Some(community) = community {
        entities.push(Entity::Described(Described {
            what: None,
            kind: SYSTEM_OBJECT,
            role: None,
            name: Some(String::from("query")),
            description: None,
            details: vec![Detail::new(HOME_COMMUNITY_DETAIL, community)],
            security_labels: Vec::new(),
        }));
    }
    entities.extend(patients.iter().map(Identifier::patient));
    call.exchange(
        EventKind {
            profile: "",
            event_type: QUERY,
            subtypes: vec![ITI_55],
            action: "E",
            client: SOURCE_ROLE,
            server: DESTINATION_ROLE,
        },
        outcome,
        entities,
    )
}

/// The `ITI-94` subtype (PMIR 1.6.0 §2:3.94.5.1, the PMIR subscription audit
/// profiles).
pub const ITI_94: Coded = Coded::fixed(IHE_TRANSACTIONS, "ITI-94", "Subscribe to Patient Updates");

/// What an ITI-94 request did to a subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionAction {
    /// Created it.
    Create,
    /// Read it.
    Read,
    /// Enabled or disabled it.
    Update,
    /// Deleted it.
    Delete,
}

/// Returns the record of an ITI-94 request that did `action` to the
/// subscription `id`, ended in `outcome` (PMIR 1.6.0 §2:3.94.5.1).
///
/// A create, a read and a delete follow the PMIR subscription audit profile
/// on BALP Create, Read and Delete; an enable or disable is a BALP Update,
/// which PMIR gives no profile of its own.
#[must_use]
pub fn subscription(
    call: &Call,
    action: SubscriptionAction,
    id: Option<&str>,
    outcome: Outcome,
) -> Exchange {
    let (profile, subtype, code, client, server) = match action {
        SubscriptionAction::Create => (
            "https://profiles.ihe.net/ITI/PMIR/StructureDefinition/IHE.PMIR.Audit.Subscription.Create",
            CREATE,
            "C",
            SOURCE_ROLE,
            DESTINATION_ROLE,
        ),
        SubscriptionAction::Read => (
            "https://profiles.ihe.net/ITI/PMIR/StructureDefinition/IHE.PMIR.Audit.Subscription.Read",
            READ,
            "R",
            DESTINATION_ROLE,
            SOURCE_ROLE,
        ),
        SubscriptionAction::Update => (
            "https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.Update",
            UPDATE,
            "U",
            SOURCE_ROLE,
            DESTINATION_ROLE,
        ),
        SubscriptionAction::Delete => (
            "https://profiles.ihe.net/ITI/PMIR/StructureDefinition/IHE.PMIR.Audit.Subscription.Delete",
            DELETE,
            "D",
            APPLICATION,
            CUSTODIAN,
        ),
    };
    call.exchange(
        EventKind {
            profile,
            event_type: REST,
            subtypes: vec![subtype, ITI_94],
            action: code,
            client,
            server,
        },
        outcome,
        vec![Entity::Resource {
            reference: id.map(|id| format!("Subscription/{id}")),
            resource_type: "Subscription",
            kind: SYSTEM_OBJECT,
            role: Some(DOMAIN_RESOURCE),
            name: None,
            query: None,
        }],
    )
}

/// The `ITI-93` subtype.
pub const ITI_93: Coded = Coded::fixed(IHE_TRANSACTIONS, "ITI-93", "Mobile Patient Identity Feed");

/// The `type` of the PMIR Feed audit profile's message entity.
const MESSAGE_HEADER: Coded = Coded::fixed(
    "http://hl7.org/fhir/resource-types",
    "MessageHeader",
    "MessageHeader",
);

/// The event a PMIR feed message's `MessageHeader` names.
const FEED_EVENT: &str = "urn:ihe:iti:pmir:2019:patient-feed";

/// Returns the record of one ITI-93 message the Registry received, ended in
/// `outcome` (PMIR 1.6.0 §2:3.93.5.1).
///
/// The record follows the PMIR Feed audit profile: one patient entity per
/// `Patient/{id}` of `patients`, and the message's `MessageHeader`, whose
/// id is `message`. A message refused before it was read names neither.
#[must_use]
pub fn pmir_feed(
    call: &Call,
    patients: Vec<SecretString>,
    message: Option<&str>,
    outcome: Outcome,
) -> Exchange {
    let mut entities: Vec<Entity> = patients.into_iter().map(Entity::PatientReference).collect();
    if let Some(id) = message {
        entities.push(Entity::Resource {
            reference: Some(format!("MessageHeader/{id}")),
            resource_type: "MessageHeader",
            kind: MESSAGE_HEADER,
            role: None,
            name: Some(FEED_EVENT),
            query: None,
        });
    }
    call.exchange(
        EventKind {
            profile: "https://profiles.ihe.net/ITI/PMIR/StructureDefinition/IHE.PMIR.Feed.Audit",
            event_type: PATIENT_RECORD,
            subtypes: vec![ITI_93],
            action: "E",
            client: SOURCE_ROLE,
            server: DESTINATION_ROLE,
        },
        outcome,
        entities,
    )
}

/// Returns the record of a pseudonym minted for the person `person`
/// (`Patient/{id}`) in the pseudonymisation domain `namespace`.
///
/// The record is BALP Create (BALP 1.1.4 §3:5.7.3): the person as the
/// patient, the domain as the data, and no pseudonym value.
#[must_use]
pub fn pseudonym_minted(call: &Call, person: &str, namespace: &str, outcome: Outcome) -> Exchange {
    call.exchange(
        EventKind {
            profile: "https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.Create",
            event_type: REST,
            subtypes: vec![CREATE],
            action: "C",
            client: SOURCE_ROLE,
            server: DESTINATION_ROLE,
        },
        outcome,
        vec![
            Entity::PatientReference(SecretString::from(person.to_owned())),
            Entity::Described(Described {
                what: None,
                kind: SYSTEM_OBJECT,
                role: Some(DOMAIN_RESOURCE),
                name: Some(String::from("pseudonym")),
                description: None,
                details: vec![Detail::new("namespace", namespace)],
                security_labels: Vec::new(),
            }),
        ],
    )
}

/// The search transaction a PDQm Supplier record is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Search {
    /// ITI-78, the Patient search.
    Query,
    /// ITI-119, `$match`.
    Match,
}

/// Returns the records of a PDQm `search` that found `persons`.
///
/// `query` is the raw request and each person a `Patient/{id}`; the search
/// ended in `outcome`. One record per person, as BALP 1.1.4 §1:52.4.1.6
/// recommends, or one with no patient when none was found (PDQm 3.2.0
/// §2:3.78.5.1.2, §2:3.119.5.1.2).
#[must_use]
pub fn pdq_search(
    call: &Call,
    search: Search,
    query: &SecretString,
    persons: &[String],
    outcome: Outcome,
) -> Vec<Exchange> {
    let (profile, subtype) = match search {
        Search::Query => (
            "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Query.Audit.Supplier",
            ITI_78,
        ),
        Search::Match => (
            "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Match.Audit.Supplier",
            ITI_119,
        ),
    };
    let kind = || EventKind {
        profile,
        event_type: REST,
        subtypes: vec![subtype.clone(), SEARCH],
        action: "E",
        client: SOURCE_ROLE,
        server: DESTINATION_ROLE,
    };
    if persons.is_empty() {
        return vec![call.exchange(kind(), outcome, vec![Entity::Query(query.clone())])];
    }
    persons
        .iter()
        .map(|person| {
            call.exchange(
                kind(),
                outcome,
                vec![
                    Entity::Query(query.clone()),
                    Entity::PatientReference(SecretString::from(person.clone())),
                ],
            )
        })
        .collect()
}

/// The `entity:data` of a write or a read of the person `person`.
pub(super) fn person_data(person: Option<&str>) -> Entity {
    Entity::Resource {
        reference: person.map(str::to_owned),
        resource_type: "Patient",
        kind: SYSTEM_OBJECT,
        role: Some(DOMAIN_RESOURCE),
        name: None,
        query: None,
    }
}
