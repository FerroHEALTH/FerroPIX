// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A described entity and a user's organisation and alternative identity:
//! written to the record toward the repository, shown by no `Debug`.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the written record is read as a JSON value"
)]

use ihe_iti::balp::{
    DESTINATION_ROLE, Described, Detail, Direction, Entity, EventKind, Exchange, OTHER, Outcome,
    Peer, READ, REQUEST_ID, REST, SOURCE_ROLE, What,
};
use ihe_iti::user::{OnBehalfOf, User};
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

#[test]
fn no_debug_shows_a_described_value_or_the_users_organisation() {
    let exchange = exchange();
    let shown = format!("{exchange:?}");
    for value in [TEMPLATE, REQUEST, ORGANISATION, PROFESSIONAL] {
        assert!(!shown.contains(value), "{value} in {shown}");
    }
    assert!(shown.contains("template-id"), "the detail's name is shown");
}
