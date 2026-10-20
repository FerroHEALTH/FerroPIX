// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Where a record carries the purposes of use of the user an exchange was
//! made for: at the `agent:user` `purposeOfUse` (BALP 1.1.4), and once each
//! at `purposeOfEvent`, the element the `RESTful` ATNA supplement (Rev. 3.6,
//! Tables 3.20.4.2.2.1-1 and 3.81.4.2.2.1-1) maps to the DICOM
//! `EventIdentification` `PurposeOfUse` that ITI TF-2 §3.20.7.1.2 asks for.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the written record and the vendored profile are read as JSON values"
)]

use std::path::PathBuf;

use ihe_iti::balp::{
    DESTINATION_ROLE, Direction, EventKind, Exchange, Outcome, Peer, READ, REST, SOURCE_ROLE,
};
use ihe_iti::user::{OnBehalfOf, PurposeOfUse, User};
use secrecy::ExposeSecret as _;
use serde_json::Value;
use url::Url;

use super::observer;

/// The BALP Read pattern the exchange is recorded under.
const READ_PROFILE: &str =
    "https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.Read";

const KIND: EventKind = EventKind {
    profile: READ_PROFILE,
    event_type: REST,
    subtypes: &[READ],
    action: "R",
    client: DESTINATION_ROLE,
    server: SOURCE_ROLE,
};

/// The HL7 v3 `ActReason` code system the synthetic purposes are drawn from.
const ACT_REASON: &str = "http://terminology.hl7.org/CodeSystem/v3-ActReason";

/// The HL7 v3 `PurposeOfUse` value set BALP binds both elements to.
const PURPOSE_OF_USE: &str = "http://terminology.hl7.org/ValueSet/v3-PurposeOfUse";

fn purpose(code: &str) -> PurposeOfUse {
    PurposeOfUse {
        system: Some(ACT_REASON.to_owned()),
        code: code.to_owned(),
    }
}

/// An exchange made for a synthetic user who declared `purposes`.
fn exchange(purposes: Vec<PurposeOfUse>) -> Exchange {
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
                "Qz7-subject-51".to_owned(),
                "Qz7-client-52".to_owned(),
            )
            .with_organisation(Some("Qz7-organisation-53".to_owned()))
            .with_purposes(purposes),
        ),
        entities: Vec::new(),
        security: Vec::new(),
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

/// The `(system, code)` of each coding of each concept in `concepts`.
fn codes(concepts: &Value) -> Vec<(String, String)> {
    concepts
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|concept| concept["coding"].as_array().into_iter().flatten())
        .map(|coding| {
            (
                coding["system"].as_str().unwrap_or_default().to_owned(),
                coding["code"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

/// The purposes at the `agent:user` (`IRCP`) of `record`.
fn at_user(record: &Value) -> Vec<(String, String)> {
    let users: Vec<&Value> = record["agent"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|agent| agent["type"]["coding"][0]["code"] == "IRCP")
        .collect();
    let [user] = users.as_slice() else {
        panic!("one agent:user, got {}", users.len());
    };
    codes(&user["purposeOfUse"])
}

fn expected(list: &[&str]) -> Vec<(String, String)> {
    list.iter()
        .map(|code| (ACT_REASON.to_owned(), (*code).to_owned()))
        .collect()
}

#[test]
fn every_purpose_is_written_at_the_user_and_at_purpose_of_event() {
    let record = written(&exchange(vec![purpose("TREAT"), purpose("BTG")]));
    assert_eq!(at_user(&record), expected(&["TREAT", "BTG"]), "agent:user");
    assert_eq!(
        codes(&record["purposeOfEvent"]),
        expected(&["TREAT", "BTG"]),
        "purposeOfEvent"
    );
}

#[test]
fn an_emergency_purpose_reaches_purpose_of_event() {
    let record = written(&exchange(vec![purpose("BTG")]));
    assert_eq!(codes(&record["purposeOfEvent"]), expected(&["BTG"]));
}

#[test]
fn a_purpose_declared_twice_appears_once_at_purpose_of_event() {
    let record = written(&exchange(vec![
        purpose("BTG"),
        purpose("TREAT"),
        purpose("BTG"),
    ]));
    assert_eq!(
        codes(&record["purposeOfEvent"]),
        expected(&["BTG", "TREAT"])
    );
}

#[test]
fn a_record_with_no_purpose_has_no_purpose_of_event() {
    let record = written(&exchange(Vec::new()));
    assert!(record["purposeOfEvent"].is_null(), "no purposeOfEvent");
    assert!(at_user(&record).is_empty(), "no agent:user purposeOfUse");
}

/// BALP 1.1.4 Read admits `purposeOfEvent` as `0..*` bound extensibly to the
/// HL7 v3 `PurposeOfUse` value set, the binding the `agent:user` purposes
/// already hold to, so writing them there keeps the record's pattern.
#[test]
fn the_read_pattern_admits_the_purposes_at_purpose_of_event() {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../docs/specs/ihe-balp/package/StructureDefinition-IHE.BasicAudit.Read.json",
    ]
    .iter()
    .collect();
    let text = std::fs::read_to_string(&path).expect("the vendored BALP Read profile");
    let profile: Value = serde_json::from_str(&text).expect("vendored JSON");
    assert_eq!(profile["url"], READ_PROFILE);
    let elements = profile["snapshot"]["element"]
        .as_array()
        .expect("a snapshot");
    let element = |id: &str| {
        elements
            .iter()
            .find(|element| element["id"] == id)
            .unwrap_or_else(|| panic!("BALP Read defines {id}"))
    };
    let event = element("AuditEvent.purposeOfEvent");
    assert_eq!(event["max"], "*", "purposeOfEvent is 0..*");
    assert_eq!(event["binding"]["valueSet"], PURPOSE_OF_USE);
    assert_eq!(event["binding"]["strength"], "extensible");
    assert_eq!(
        element("AuditEvent.agent.purposeOfUse")["binding"]["valueSet"],
        PURPOSE_OF_USE
    );
}
