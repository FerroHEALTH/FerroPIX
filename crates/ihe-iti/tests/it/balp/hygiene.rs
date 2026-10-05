// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A record names the patient toward the Audit Record Repository only: no
//! `Debug` of an exchange, an entity or a written record shows an identifier,
//! a patient reference or the request that carries them (§5.4, N33, as the
//! gateway holds it for every surface but the repository).

use ihe_iti::balp::{
    DESTINATION_ROLE, Direction, Entity, EventKind, Exchange, Outcome, Peer, REST, SEARCH,
    SOURCE_ROLE,
};
use secrecy::{ExposeSecret as _, SecretString};
use url::Url;

use super::observer;

const KIND: EventKind = EventKind {
    profile: "https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.PatientQuery",
    event_type: REST,
    subtypes: &[SEARCH],
    action: "E",
    client: SOURCE_ROLE,
    server: DESTINATION_ROLE,
};

/// The synthetic values the exchange carries, each unlike any other text.
const VALUE: &str = "Qz7-value-17";
const REFERENCE: &str = "Patient/Qz7-ref-18";
const SEARCHED: &str = "Qz7-search-19";

fn exchange() -> Exchange {
    Exchange {
        kind: KIND,
        recorded: jiff::Timestamp::UNIX_EPOCH,
        outcome: Outcome::Success,
        direction: Direction::Sent {
            server: Peer::server(
                &Url::parse("https://user:Qz7-password@pix.example.org/fhir/?token=Qz7-token")
                    .expect("a URL"),
            ),
        },
        on_behalf: ihe_iti::user::OnBehalfOf::System,
        entities: vec![
            Entity::Query(SecretString::from(format!(
                "https://pix.example.org/fhir/Patient?identifier={SEARCHED}"
            ))),
            Entity::Patient {
                system: "urn:oid:2.999.1".to_owned(),
                value: SecretString::from(VALUE),
            },
            Entity::PatientReference(SecretString::from(REFERENCE)),
        ],
    }
}

#[test]
fn no_debug_shows_a_patient_or_the_request() {
    let exchange = exchange();
    let record = exchange.audit_event(&observer()).expect("a record");
    for shown in [format!("{exchange:?}"), format!("{record:?}")] {
        for value in [VALUE, REFERENCE, SEARCHED, "Qz7-password", "Qz7-token"] {
            assert!(!shown.contains(value), "{value} in {shown}");
        }
    }
}

#[cfg(any(feature = "pixm", feature = "pdqm", feature = "mcsd", feature = "pmir"))]
#[test]
fn no_debug_shows_the_user_and_the_record_names_them_toward_the_repository() {
    use super::profile::{AUDIENCE, CLIENT, ISSUER, SUBJECT, user};
    let exchange = Exchange {
        on_behalf: user(),
        ..exchange()
    };
    let record = exchange.audit_event(&observer()).expect("a record");
    let shown = format!("{exchange:?} {record:?} {:?}", user());
    for value in [SUBJECT, CLIENT, ISSUER, AUDIENCE] {
        assert!(!shown.contains(value), "{value} in {shown}");
    }
    let text = String::from_utf8_lossy(record.into_bytes().expose_secret()).into_owned();
    for value in [SUBJECT, CLIENT, ISSUER] {
        assert!(text.contains(value), "{value} reaches the repository");
    }
}

#[test]
fn the_record_carries_the_patient_and_no_server_credential() {
    let record = exchange()
        .audit_event(&observer())
        .expect("a record")
        .into_bytes();
    let text = String::from_utf8_lossy(record.expose_secret()).into_owned();
    assert!(
        text.contains(VALUE),
        "the identifier reaches the repository"
    );
    assert!(text.contains(REFERENCE));
    for credential in ["Qz7-password", "Qz7-token"] {
        assert!(
            !text.contains(credential),
            "the server is named without its userinfo or query"
        );
    }
}
