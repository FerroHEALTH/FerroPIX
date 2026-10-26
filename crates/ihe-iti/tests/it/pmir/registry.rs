// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Patient Identity Registry side of ITI-93 and ITI-94 (feature
//! `pmir-server`): the vendored PMIR message examples read in JSON and XML
//! with their Patients, the messages and subscriptions it refuses, the
//! response it writes, and the outbound message read back by the subscriber
//! side.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the vendored examples are turned into FHIR XML as JSON values"
)]

use fhir_types::codec::{Object, Value};
use fhir_types::r4::message_header::MessageHeaderEvent;
use fhir_types::r4::patient::{Patient, PatientLink};
use fhir_types::r4::reference::Reference;
use fhir_types::r4::resource::Resource;
use http::StatusCode;
use ihe_iti::outcome::IssueType;
use ihe_iti::pmir::error::FeedError;
use ihe_iti::pmir::feed::{EventKind, Feed};
use ihe_iti::pmir::server::feed::{Applied, Change, FeedMessage, ReadError, response};
use ihe_iti::pmir::server::notification::{Addressing, Notice, message};
use ihe_iti::pmir::server::subscription::{Criteria, RequestedSubscription, SubscriptionError};
use ihe_iti::server::format::Format;
use url::Url;

use super::{FHIR_JSON, vendored};

/// The resource `json` holds, as FHIR XML.
fn xml(json: &str) -> String {
    let value: Value = serde_json::from_str(json).expect("JSON");
    let object: &Object = value.as_object().expect("a resource");
    fhir_types::xml::to_xml(&fhir_types::r4::schema::SCHEMAS, object).expect("an XML form")
}

/// The vendored example `file` in `format`.
fn body(file: &str, format: Format) -> Vec<u8> {
    let json = vendored(file);
    match format {
        Format::Xml => xml(&json).into_bytes(),
        _ => json.into_bytes(),
    }
}

/// The ids of the Patients `changes` carry, with the kind of each change.
fn kinds(changes: &[Change]) -> Vec<(String, Option<String>)> {
    changes
        .iter()
        .map(|change| match change {
            Change::Create(patient) => (String::from("create"), patient.id.clone()),
            Change::Update(patient) => (String::from("update"), patient.id.clone()),
            Change::Delete { id, .. } => (String::from("delete"), Some(id.clone())),
            Change::Merge { deprecated, .. } => (String::from("merge"), deprecated.id.clone()),
            _ => (String::from("other"), None),
        })
        .collect()
}

#[test]
fn the_vendored_messages_read_with_their_patients_in_json_and_xml() {
    for format in [Format::Json, Format::Xml] {
        let read = |file: &str| {
            FeedMessage::read(format, &body(file, format))
                .unwrap_or_else(|error| panic!("{file} in {format:?}: {error}"))
        };
        let created = read("example/Bundle-ex-PMIRBundleCreate.json");
        assert_eq!("ex-messageheader-create", created.id());
        assert_eq!("http://example.com/patientSource", created.source());
        assert_eq!(
            vec![
                (
                    String::from("create"),
                    Some(String::from("ex-patient-create1"))
                ),
                (
                    String::from("create"),
                    Some(String::from("ex-patient-create2"))
                ),
            ],
            kinds(created.changes())
        );
        let Change::Create(patient) = &created.changes()[0] else {
            panic!("a create");
        };
        assert!(
            !patient.name.is_empty(),
            "the Patient keeps its demographics"
        );
        assert_eq!(
            vec![(
                String::from("update"),
                Some(String::from("ex-patient-update"))
            )],
            kinds(read("example/Bundle-ex-PMIRBundleUpdate.json").changes())
        );
        assert_eq!(
            vec![(
                String::from("delete"),
                Some(String::from("ex-patient-delete"))
            )],
            kinds(read("example/Bundle-ex-PMIRBundleDelete.json").changes())
        );
        let merged = read("example/Bundle-ex-PMIRBundleMerge.json");
        let [Change::Merge { surviving, .. }] = merged.changes() else {
            panic!("one merge");
        };
        assert_eq!("Patient/ex-patient-merged", surviving);
    }
}

#[test]
fn a_message_without_its_header_first_or_a_merge_off_its_profile_is_refused() {
    let mut value: serde_json::Value =
        serde_json::from_str(&vendored("example/Bundle-ex-PMIRBundleCreate.json")).expect("JSON");
    let entries = value
        .get_mut("entry")
        .and_then(serde_json::Value::as_array_mut)
        .expect("entries");
    entries.swap(0, 1);
    let swapped = serde_json::to_vec(&value).expect("JSON");
    let refused = FeedMessage::read(Format::Json, &swapped).expect_err("no header first");
    assert!(matches!(refused, ReadError::Message(_)), "{refused:?}");
    assert_eq!(StatusCode::BAD_REQUEST, refused.refusal().status());

    let unlinked =
        vendored("example/Bundle-ex-PMIRBundleMerge.json").replace("replaced-by", "seealso");
    let read = FeedMessage::read(Format::Json, unlinked.as_bytes()).expect("an update");
    assert!(
        matches!(read.changes(), [Change::Update(_)]),
        "with no replaced-by link the entry is no merge (§2:3.93.4.1.2.4)"
    );
    let created = vendored("example/Bundle-ex-PMIRBundleMerge.json")
        .replace(r#""method":"PUT""#, r#""method":"POST""#)
        .replace(r#""url":"Patient/ex-patient-merge""#, r#""url":"Patient""#);
    let refused = FeedMessage::read(Format::Json, created.as_bytes()).expect_err("a POST merge");
    assert!(
        matches!(refused, ReadError::Message(FeedError::Merge { .. })),
        "a merge is sent as an update: {refused:?}"
    );
    let unmerged = vendored("example/Bundle-ex-PMIRBundleMerge.json")
        .replace(r#""active":false"#, r#""active":true"#);
    assert!(
        FeedMessage::read(Format::Json, unmerged.as_bytes()).is_err(),
        "a merged Patient that is still active departs from the merged Patient profile"
    );
    let garbled = FeedMessage::read(Format::Xml, b"<Bundle").expect_err("no XML");
    assert!(matches!(garbled, ReadError::Body(_)));
    assert_eq!(IssueType::Structure, garbled.refusal().issue_type());
}

#[test]
fn the_response_names_the_message_and_correlates_a_refused_unmerge() {
    let read = FeedMessage::read(
        Format::Json,
        vendored("example/Bundle-ex-PMIRBundleMerge.json").as_bytes(),
    )
    .expect("reads");
    let ok = response(
        &read,
        "r-1",
        "https://pmir.example.org/fhir",
        &[Applied::Applied],
    );
    assert_eq!(StatusCode::OK, ok.status());
    let bundle = ok.resource();
    assert_eq!(Some("message"), bundle.r#type.value.as_deref());
    let [entry] = bundle.entry.as_slice() else {
        panic!("one entry");
    };
    let Some(Resource::MessageHeader(header)) = &entry.resource else {
        panic!("a MessageHeader");
    };
    assert!(
        matches!(&header.event, MessageHeaderEvent::Uri(uri) if uri.value.as_deref() == Some("urn:ihe:iti:pmir:2019:patient-feed-response"))
    );
    assert!(
        header.destination.is_empty(),
        "the response profile has none"
    );
    let answered = header.response.as_ref().expect("a response");
    assert_eq!(
        Some("ex-messageheader-merge"),
        answered.identifier.value.as_deref()
    );
    assert_eq!(Some("ok"), answered.code.value.as_deref());

    let refused = response(
        &read,
        "r-2",
        "https://pmir.example.org/fhir",
        &[Applied::Refused {
            status: StatusCode::METHOD_NOT_ALLOWED,
            issue: IssueType::NotSupported,
            diagnostics: "an unmerge is not supported",
        }],
    );
    let Some(Resource::MessageHeader(header)) = &refused.resource().entry[0].resource else {
        panic!("a MessageHeader");
    };
    let answered = header.response.as_ref().expect("a response");
    assert_eq!(Some("fatal-error"), answered.code.value.as_deref());
    assert_eq!(
        Some("#outcome"),
        answered
            .details
            .as_ref()
            .and_then(|details| details.reference.as_ref())
            .and_then(|reference| reference.value.as_deref())
    );
    let [Resource::OperationOutcome(outcome)] = header.contained.as_slice() else {
        panic!("the contained outcome");
    };
    assert_eq!(
        Some("405: an unmerge is not supported"),
        outcome.issue[0]
            .diagnostics
            .as_ref()
            .and_then(|d| d.value.as_deref())
    );
    assert_eq!(
        Some("Bundle.entry[1].resource.entry[0]"),
        outcome.issue[0]
            .expression
            .first()
            .and_then(|e| e.value.as_deref())
    );
}

#[test]
fn the_vendored_subscription_reads_with_its_criteria() {
    for format in [Format::Json, Format::Xml] {
        for file in [
            "example/Subscription-ex-subscription-request.json",
            "example/Subscription-ex-subscription.json",
        ] {
            let read = RequestedSubscription::read(format, &body(file, format))
                .unwrap_or_else(|error| panic!("{file} in {format:?}: {error}"));
            assert_eq!(
                "message",
                read.resource()
                    .channel
                    .r#type
                    .value
                    .as_deref()
                    .unwrap_or_default()
            );
            assert_ne!("", read.payload());
            assert!(read.endpoint().scheme().starts_with("http"));
        }
    }
}

#[test]
fn each_of_the_four_criteria_reads_and_any_other_is_refused() {
    assert!(matches!(Criteria::parse("Patient"), Ok(Criteria::All)));
    assert!(matches!(Criteria::parse("Patient?_id=p1"), Ok(Criteria::Id(id)) if id == "p1"));
    assert!(matches!(
        Criteria::parse("Patient?organization=Organization/o1"),
        Ok(Criteria::Organization(org)) if org == "Organization/o1"
    ));
    let Ok(Criteria::Identifier { system, value }) =
        Criteria::parse("Patient?identifier=urn:oid:2.999.1%7CA%5C%7CB")
    else {
        panic!("an identifier");
    };
    assert_eq!(Some("urn:oid:2.999.1"), system.as_deref());
    assert_eq!(
        Some("A|B"),
        value.as_ref().map(secrecy::ExposeSecret::expose_secret),
        "the escape is undone"
    );
    for refused in [
        "Patient?name=Smith",
        "Observation",
        "Patient?_id=p1&_id=p2",
        "Patient?identifier=",
        "Patients",
    ] {
        assert!(
            matches!(Criteria::parse(refused), Err(SubscriptionError::Criteria)),
            "{refused}"
        );
    }
    let mut subscription: serde_json::Value = serde_json::from_str(&vendored(
        "example/Subscription-ex-subscription-request.json",
    ))
    .expect("JSON");
    subscription["criteria"] = serde_json::Value::String(String::from("Patient?gender=female"));
    let refused = RequestedSubscription::read(
        Format::Json,
        &serde_json::to_vec(&subscription).expect("JSON"),
    )
    .expect_err("a fifth criteria");
    assert_eq!(SubscriptionError::Criteria, refused);
    assert_eq!(IssueType::NotSupported, refused.refusal().issue_type());
}

#[test]
fn a_criteria_matches_the_patients_a_search_would_find() {
    let patient: Patient = serde_json::from_str(
        r#"{"resourceType":"Patient","id":"p1","identifier":[{"system":"urn:oid:2.999.1","value":"A-1"}],"managingOrganization":{"reference":"Organization/o1"}}"#,
    )
    .expect("a Patient");
    for (criteria, want) in [
        ("Patient", true),
        ("Patient?_id=p1", true),
        ("Patient?_id=p2", false),
        ("Patient?organization=o1", true),
        ("Patient?organization=Organization/o1", true),
        ("Patient?organization=o2", false),
        ("Patient?identifier=urn:oid:2.999.1%7CA-1", true),
        ("Patient?identifier=urn:oid:2.999.1%7C", true),
        ("Patient?identifier=A-1", true),
        ("Patient?identifier=%7CA-1", false),
        ("Patient?identifier=urn:oid:2.999.2%7CA-1", false),
    ] {
        assert_eq!(
            want,
            Criteria::parse(criteria).expect("reads").matches(&patient),
            "{criteria}"
        );
    }
}

#[test]
fn the_outbound_message_reads_back_through_the_subscriber() {
    let patient = |id: &str| -> Box<Patient> {
        Box::new(
            serde_json::from_str(&format!(
                r#"{{"resourceType":"Patient","id":"{id}","identifier":[{{"system":"urn:oid:2.999.1","value":"V-{id}"}}],"active":true}}"#
            ))
            .expect("a Patient"),
        )
    };
    let mut merged = patient("old");
    merged.active = Some(false.into());
    merged.link = vec![PatientLink {
        other: Reference {
            reference: Some("Patient/new".into()),
            ..Reference::default()
        },
        r#type: "replaced-by".into(),
        ..PatientLink::default()
    }];
    let registry = Url::parse("https://pmir.example.org/fhir/").expect("a URL");
    let destination = Url::parse("https://consumer.example.org/feed").expect("a URL");
    let bundle = message(
        &[
            Notice::Created(patient("a")),
            Notice::Updated(patient("b")),
            Notice::Merged(merged),
            Notice::Deleted(String::from("c")),
        ],
        &Addressing {
            message_id: "7f4b3c2a-0000-4000-8000-000000000093",
            history_id: "7f4b3c2a-0000-4000-8000-000000000094",
            registry: &registry,
            destination: &destination,
        },
    );
    let json = serde_json::to_vec(&bundle).expect("JSON");
    let feed = Feed::read(Some(FHIR_JSON), &json).expect("the subscriber reads it");
    assert_eq!("7f4b3c2a-0000-4000-8000-000000000093", feed.message_id());
    assert_eq!(
        [1, 1, 1, 1],
        [
            feed.count(EventKind::Create),
            feed.count(EventKind::Update),
            feed.count(EventKind::Merge),
            feed.count(EventKind::Delete),
        ]
    );
    let back = FeedMessage::read(Format::Json, &json).expect("the Registry side reads it too");
    assert_eq!("https://pmir.example.org/fhir", back.source());
}
