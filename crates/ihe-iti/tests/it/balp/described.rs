// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A described entity and a user's organisation, alternative identity,
//! name, provider identifier, roles and assurance level:
//! written to the record toward the repository, shown by no `Debug`.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the written record is read as a JSON value"
)]

use ihe_iti::balp::{
    DESTINATION_ROLE, Described, Detail, Direction, Entity, EventKind, Exchange, OTHER, Outcome,
    Peer, READ, REQUEST_ID, REST, SOURCE_ROLE, What,
};
use ihe_iti::user::{Code, OnBehalfOf, User};
use secrecy::ExposeSecret as _;
use serde_json::Value;
use url::Url;

use super::observer;

const KIND: EventKind = EventKind {
    profile: "https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.Read",
    event_type: REST,
    subtypes: &[READ],
    action: "R",
    client: DESTINATION_ROLE,
    server: SOURCE_ROLE,
};

/// The synthetic values the exchange carries, each unlike any other text.
const TEMPLATE: &str = "Qz7-template-31";
const REQUEST: &str = "Qz7-request-32";
const ORGANISATION: &str = "Qz7-organisation-33";
const PROFESSIONAL: &str = "Qz7-professional-34";
const NAME: &str = "Qz7-name-37";
const PROVIDER_ID: &str = "Qz7-provider-id-38";
const ROLE: &str = "Qz7-role-39";
const LEVEL: &str = "Qz7-level-40";

/// A synthetic code system of assurance levels.
const LEVELS: &str = "urn:example:assurance";

fn exchange() -> Exchange {
    Exchange {
        kind: KIND,
        recorded: jiff::Timestamp::UNIX_EPOCH,
        outcome: Outcome::Success,
        direction: Direction::Received {
            client: Peer::server(&Url::parse("https://client.example.org/").expect("a URL")),
            endpoint: Url::parse("https://gateway.example.org/v1/ehr").expect("a URL"),
        },
        on_behalf: OnBehalfOf::User(
            User::new(
                "https://issuer.example.org".to_owned(),
                "Qz7-subject-35".to_owned(),
                "Qz7-client-36".to_owned(),
            )
            .with_organisation(Some(ORGANISATION.to_owned()))
            .with_alt_id(Some(PROFESSIONAL.to_owned())),
        ),
        entities: vec![
            Entity::Described(Described {
                what: Some(What::Identifier {
                    system: None,
                    value: REQUEST.to_owned(),
                }),
                kind: REQUEST_ID,
                role: None,
                name: None,
                description: None,
                details: Vec::new(),
            }),
            Entity::Described(Described {
                what: None,
                kind: OTHER,
                role: None,
                name: Some("category".to_owned()),
                description: None,
                details: vec![Detail::new("template-id", TEMPLATE)],
            }),
        ],
    }
}

fn written(exchange: &Exchange) -> Value {
    let record = exchange
        .audit_event(&observer())
        .expect("a record")
        .into_bytes();
    let bytes = record.expose_secret();
    serde_json::from_slice::<fhir_types::r4::audit_event::AuditEvent>(bytes)
        .expect("the record reads back as an R4 AuditEvent");
    serde_json::from_slice(bytes).expect("JSON")
}

#[test]
fn a_described_entity_is_written_with_its_identifier_type_and_details() {
    let record = written(&exchange());
    let entities = record["entity"].as_array().expect("entities");
    assert_eq!(entities[0]["what"]["identifier"]["value"], REQUEST);
    assert_eq!(entities[0]["type"]["code"], "XrequestId");
    assert_eq!(entities[1]["type"]["code"], "4");
    assert_eq!(entities[1]["name"], "category");
    assert_eq!(entities[1]["detail"][0]["type"], "template-id");
    assert_eq!(entities[1]["detail"][0]["valueString"], TEMPLATE);
}

#[test]
fn the_user_is_written_with_the_organisation_and_alternative_identity() {
    let record = written(&exchange());
    let agents = record["agent"].as_array().expect("agents");
    let user = agents
        .iter()
        .find(|agent| agent["type"]["coding"][0]["code"] == "IRCP")
        .expect("the user agent");
    assert_eq!(user["altId"], PROFESSIONAL);
    let organisation = agents
        .iter()
        .find(|agent| agent["who"]["type"] == "Organization")
        .expect("the organisation agent");
    assert_eq!(organisation["who"]["identifier"]["value"], ORGANISATION);
    assert_eq!(organisation["requestor"], false);
    assert!(organisation.get("type").is_none(), "it fills no BALP slice");
}

/// The exchange of [`exchange`], its user stated with a name, a provider
/// identifier, a role and an assurance level.
fn with_professional() -> Exchange {
    let mut exchange = exchange();
    let OnBehalfOf::User(user) = exchange.on_behalf else {
        panic!("the exchange is made for a user");
    };
    exchange.on_behalf = OnBehalfOf::User(
        user.with_name(Some(NAME.to_owned()))
            .with_provider_identifier(Some(PROVIDER_ID.to_owned()))
            .with_roles(vec![Code {
                system: None,
                code: ROLE.to_owned(),
            }])
            .with_assurance(Some(Code {
                system: Some(LEVELS.to_owned()),
                code: LEVEL.to_owned(),
            })),
    );
    exchange
}

/// The `agent:user` of `record`.
fn user_agent(record: &Value) -> &Value {
    record["agent"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| agent["type"]["coding"][0]["code"] == "IRCP")
        .expect("the user agent")
}

/// BALP 1.1.4 §3:5.7.5.4: the IUA `subject_name` is `agent[user].who.display`
/// and `national_provider_identifier` is `agent[user].extension[otherId][npi]`,
/// written as the `AuditEvent-ex-auditPoke-SAML-Comp` example writes it.
#[test]
fn the_users_name_and_provider_identifier_are_written_as_balp_maps_them() {
    let record = written(&with_professional());
    let user = user_agent(&record);
    assert_eq!(user["who"]["display"], NAME);
    assert_eq!(user["who"]["identifier"]["value"], "Qz7-subject-35");
    let other_ids: Vec<&Value> = user["extension"]
        .as_array()
        .expect("extensions")
        .iter()
        .filter(|extension| {
            extension["url"] == "https://profiles.ihe.net/ITI/BALP/StructureDefinition/ihe-otherId"
        })
        .collect();
    let [npi] = other_ids.as_slice() else {
        panic!("one otherId, got {other_ids:?}");
    };
    let coding = &npi["valueIdentifier"]["type"]["coding"][0];
    assert_eq!(
        coding["system"],
        "http://terminology.hl7.org/CodeSystem/v2-0203"
    );
    assert_eq!(coding["code"], "NPI", "OtherIdentifierTypesVS");
    assert_eq!(npi["valueIdentifier"]["value"], PROVIDER_ID);
}

/// The `ihe-assuranceLevel` extension (context `AuditEvent.agent`) carries
/// the level as a `CodeableConcept`, and `agent.role` the user's roles.
#[test]
fn the_users_assurance_level_and_roles_are_written_on_the_user_agent() {
    let record = written(&with_professional());
    let user = user_agent(&record);
    let levels: Vec<&Value> = user["extension"]
        .as_array()
        .expect("extensions")
        .iter()
        .filter(|extension| {
            extension["url"]
                == "https://profiles.ihe.net/ITI/BALP/StructureDefinition/ihe-assuranceLevel"
        })
        .collect();
    let [level] = levels.as_slice() else {
        panic!("one assurance level, got {levels:?}");
    };
    assert_eq!(level["valueCodeableConcept"]["coding"][0]["system"], LEVELS);
    assert_eq!(level["valueCodeableConcept"]["coding"][0]["code"], LEVEL);
    assert_eq!(user["role"][0]["coding"][0]["code"], ROLE);
    assert!(
        user["role"][0]["coding"][0].get("system").is_none(),
        "a code no system defines names none"
    );
}

#[test]
fn a_user_stated_with_none_of_them_is_written_without_them() {
    let record = written(&exchange());
    let user = user_agent(&record);
    for absent in ["extension", "role"] {
        assert!(user.get(absent).is_none(), "{absent}: {user}");
    }
    assert!(user["who"].get("display").is_none(), "{user}");
}

#[test]
fn no_debug_shows_a_described_value_or_the_users_organisation() {
    let exchange = with_professional();
    let shown = format!("{exchange:?}");
    for value in [
        TEMPLATE,
        REQUEST,
        ORGANISATION,
        PROFESSIONAL,
        NAME,
        PROVIDER_ID,
        ROLE,
        LEVEL,
    ] {
        assert!(!shown.contains(value), "{value} in {shown}");
    }
    assert!(shown.contains("template-id"), "the detail's name is shown");
}
