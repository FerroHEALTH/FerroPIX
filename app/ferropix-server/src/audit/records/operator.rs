// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The records of the operator API: a steward's view and decision, a
//! steward's search, an export of the trail, a reveal, and an
//! administrator's write of a domain.

use ihe_atna::balp::{
    CREATE, Coded, DESTINATION_ROLE, Described, Detail, EXPORT, Entity, EventKind, Exchange,
    Outcome, READ, REST, SEARCH, SOURCE_ROLE, SYSTEM_OBJECT, UPDATE,
};
use secrecy::SecretString;

use super::{Call, person_data};

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

/// Returns the record of an administrator's write of the pseudonymisation
/// domain `namespace`, a creation when `created`, ended in `outcome`; a
/// system object, as an identifier domain's write is.
#[must_use]
pub fn pseudonym_domain_write(
    call: &Call,
    created: bool,
    namespace: &str,
    outcome: Outcome,
) -> Exchange {
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
            name: Some(String::from("pseudonym_domain")),
            description: None,
            details: vec![Detail::new("namespace", namespace)],
            security_labels: Vec::new(),
        })],
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
