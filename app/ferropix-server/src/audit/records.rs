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
//! - A steward action: a `RESTful` activity on the person (BALP 1.1.4
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
    APPLICATION, CREATE, CUSTODIAN, Coded, DELETE, DESTINATION_ROLE, DOMAIN_RESOURCE, Described,
    Detail, Direction, EXPORT, Entity, EventKind, Exchange, IHE_TRANSACTIONS, NetworkAddress,
    Outcome, Peer, READ, REQUEST_ID, REST, SEARCH, SOURCE_ROLE, SYSTEM_OBJECT, UPDATE, What,
    request_text,
};
use ihe_atna::user::{OnBehalfOf, User};
use secrecy::{ExposeSecret, SecretString};

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

/// A steward action on a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StewardAction {
    /// The person's record was viewed.
    View,
    /// A link, not-a-match or defer decision, a merge or an unmerge changed
    /// the person.
    Decide,
}

/// Returns the record of a steward action on `person` (`Patient/{id}`),
/// ended in `outcome`: a `RESTful` read or update (BALP 1.1.4 §3:5.7.3).
#[must_use]
pub fn steward(call: &Call, action: StewardAction, person: &str, outcome: Outcome) -> Exchange {
    let (profile, subtype, code, client, server) = match action {
        StewardAction::View => (
            "https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.PatientRead",
            READ,
            "R",
            DESTINATION_ROLE,
            SOURCE_ROLE,
        ),
        StewardAction::Decide => (
            "https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.PatientUpdate",
            UPDATE,
            "U",
            SOURCE_ROLE,
            DESTINATION_ROLE,
        ),
    };
    call.exchange(
        EventKind {
            profile,
            event_type: REST,
            subtypes: vec![subtype],
            action: code,
            client,
            server,
        },
        outcome,
        vec![
            person_data(Some(person)),
            Entity::PatientReference(SecretString::from(person.to_owned())),
        ],
    )
}

/// Returns the records of a steward's search `query`, its raw request,
/// that found `persons` (`Patient/{id}` each), ended in `outcome`.
///
/// One BALP Patient Query record per person found, or one BALP Query record
/// with no patient when none was (BALP 1.1.4 §1:52.4.1.6, §3:5.7.3).
#[must_use]
pub fn steward_search(
    call: &Call,
    query: &SecretString,
    persons: &[String],
    outcome: Outcome,
) -> Vec<Exchange> {
    let kind = |profile| EventKind {
        profile,
        event_type: REST,
        subtypes: vec![SEARCH],
        action: "E",
        client: SOURCE_ROLE,
        server: DESTINATION_ROLE,
    };
    if persons.is_empty() {
        return vec![call.exchange(
            kind("https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.Query"),
            outcome,
            vec![Entity::Query(query.clone())],
        )];
    }
    persons
        .iter()
        .map(|person| {
            call.exchange(
                kind(
                    "https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.PatientQuery",
                ),
                outcome,
                vec![
                    Entity::Query(query.clone()),
                    Entity::PatientReference(SecretString::from(person.clone())),
                ],
            )
        })
        .collect()
}

/// Returns the record of an export of the steward trail of `rows` actions,
/// filtered to the person `person` (`Patient/{id}`) when the filter names
/// one, ended in `outcome`.
///
/// An Export event (DICOM `110106`) the system sends to the steward; no BALP
/// pattern describes a trail export, so the record claims no profile (no
/// specification governs this: our own design).
#[must_use]
pub fn export(call: &Call, rows: usize, person: Option<&str>, outcome: Outcome) -> Exchange {
    let mut entities = vec![Entity::Described(Described {
        what: None,
        kind: SYSTEM_OBJECT,
        role: None,
        name: Some(String::from("steward_action")),
        description: Some(String::from("the steward audit trail")),
        details: vec![Detail::new("rows", rows.to_string())],
        security_labels: Vec::new(),
    })];
    if let Some(person) = person {
        entities.push(Entity::PatientReference(SecretString::from(
            person.to_owned(),
        )));
    }
    call.exchange(
        EventKind {
            profile: "",
            event_type: EXPORT,
            subtypes: vec![READ],
            action: "R",
            client: DESTINATION_ROLE,
            server: SOURCE_ROLE,
        },
        outcome,
        entities,
    )
}

/// The `disclose` subtype of a privacy disclosure (BALP 1.1.4
/// `IHE.BasicAudit.PrivacyDisclosure.Source`, `subtype:disclosure`).
pub const DISCLOSE: Coded = Coded::fixed(
    "http://terminology.hl7.org/CodeSystem/iso-21089-lifecycle",
    "disclose",
    "Disclose Record Lifecycle Event",
);

/// What a reveal disclosed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Disclosed<'a> {
    /// `identifier` or `pseudonym`.
    pub kind: &'static str,
    /// The identifier's domain system, or the pseudonymisation domain's
    /// namespace.
    pub system: &'a str,
    /// The steward's reason.
    pub reason: &'a str,
}

impl std::fmt::Debug for Disclosed<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Disclosed")
            .field("kind", &self.kind)
            .field("system", &self.system)
            .finish_non_exhaustive()
    }
}

/// Returns the record of a steward's reveal, a privacy disclosure at the
/// source.
///
/// The reveal is of `disclosed` of the person `person` (`Patient/{id}`),
/// ended in `outcome` (BALP 1.1.4 §3:5.7.7,
/// `IHE.BasicAudit.PrivacyDisclosure.Source`).
///
/// FerroPIX is the source agent and the steward's client the recipient; the
/// patient is the subject (§1:52.4.1.6) and the disclosed data an entity
/// naming its kind and domain, never its value. `purposeOfEvent` carries the
/// coded purposes of the steward's token, and the steward's reason, which is
/// free text, is a `reason` detail of the data entity (no specification
/// governs the reason's place: our own design).
#[must_use]
pub fn disclosure(
    call: &Call,
    person: &str,
    disclosed: Disclosed<'_>,
    outcome: Outcome,
) -> Exchange {
    call.exchange(
        EventKind {
            profile: "https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.PrivacyDisclosure.Source",
            event_type: EXPORT,
            subtypes: vec![DISCLOSE, READ],
            action: "R",
            client: DESTINATION_ROLE,
            server: SOURCE_ROLE,
        },
        outcome,
        vec![
            Entity::PatientReference(SecretString::from(person.to_owned())),
            Entity::Described(Described {
                what: None,
                kind: SYSTEM_OBJECT,
                role: None,
                name: Some(disclosed.kind.to_owned()),
                description: None,
                details: vec![
                    Detail::new("system", disclosed.system),
                    Detail::new("reason", disclosed.reason),
                ],
                security_labels: Vec::new(),
            }),
        ],
    )
}

/// Returns the record of an administrator's write of the identifier domain
/// `system`, a creation when `created`, ended in `outcome`.
///
/// A `RESTful` create or update of a system object that names no patient; no
/// BALP pattern describes a domain, so the record claims no profile (no
/// specification governs this: our own design).
#[must_use]
pub fn domain_write(call: &Call, created: bool, system: &str, outcome: Outcome) -> Exchange {
    let (subtype, code) = if created {
        (CREATE, "C")
    } else {
        (UPDATE, "U")
    };
    call.exchange(
        EventKind {
            profile: "",
            event_type: REST,
            subtypes: vec![subtype],
            action: code,
            client: SOURCE_ROLE,
            server: DESTINATION_ROLE,
        },
        outcome,
        vec![Entity::Described(Described {
            what: None,
            kind: SYSTEM_OBJECT,
            role: None,
            name: Some(String::from("identifier_domain")),
            description: None,
            details: vec![Detail::new("system", system)],
            security_labels: Vec::new(),
        })],
    )
}

/// The `entity:data` of a write or a read of the person `person`.
fn person_data(person: Option<&str>) -> Entity {
    Entity::Resource {
        reference: person.map(str::to_owned),
        resource_type: "Patient",
        kind: SYSTEM_OBJECT,
        role: Some(DOMAIN_RESOURCE),
        name: None,
        query: None,
    }
}
