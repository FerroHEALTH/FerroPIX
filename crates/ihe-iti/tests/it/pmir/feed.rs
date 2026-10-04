// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-93 rules a message is held to, one refusal each, and the changes a
//! message that holds to them reads as (§2:3.93.4.1.2; the PMIR Bundle,
//! `MessageHeader`, history Bundle and merged Patient profiles).

use ihe_iti::outcome::IssueType;
use ihe_iti::pmir::error::FeedError;
use ihe_iti::pmir::feed::{Event, EventKind, Feed, refusal};
use secrecy::ExposeSecret;

use super::{DOMAIN, FHIR_JSON, HEADER, entry, merged, message, message_with, patient};

/// A value that stands for a patient identifier.
const VALUE: &str = "SYNTHETIC-1001";

fn read(body: &str) -> Result<Feed, FeedError> {
    Feed::read(Some(FHIR_JSON), body.as_bytes())
}

fn update(id: &str) -> String {
    entry(
        "PUT",
        &format!("Patient/{id}"),
        Some(&patient(id, VALUE, "")),
        "200",
    )
}

#[test]
fn a_merge_reads_as_the_subsumed_identity_and_the_surviving_reference() {
    let body = message(&[entry(
        "PUT",
        "Patient/p-old",
        Some(&merged("p-old", VALUE, "p-new")),
        "200 OK",
    )]);
    let feed = read(&body).expect("a merge");
    let [
        Event::Merged {
            subsumed,
            surviving,
        },
    ] = feed.events()
    else {
        panic!("one merge");
    };
    assert_eq!("Patient/p-new", surviving.expose_secret());
    let [identifier] = subsumed.identifiers() else {
        panic!("one identifier");
    };
    assert_eq!(Some(DOMAIN), identifier.system());
    assert_eq!(VALUE, identifier.value().expose_secret());
    assert_eq!(1, feed.count(EventKind::Merge));
}

#[test]
fn creates_updates_and_deletes_read_in_history_order() {
    let body = message(&[
        entry(
            "POST",
            "Patient",
            Some(&patient("p-1", VALUE, "")),
            "201 Created",
        ),
        update("p-2"),
        entry("DELETE", "Patient/p-3", None, "204"),
    ]);
    let feed = read(&body).expect("three changes");
    let kinds: Vec<EventKind> = feed.events().iter().map(Event::kind).collect();
    assert_eq!(
        vec![EventKind::Create, EventKind::Update, EventKind::Delete],
        kinds
    );
}

#[test]
fn a_delete_that_carries_its_patient_keeps_the_identifiers() {
    let body = message(&[entry(
        "DELETE",
        "Patient/p-3",
        Some(&patient("p-3", VALUE, "")),
        "200",
    )]);
    let feed = read(&body).expect("a delete");
    let [Event::Deleted(deleted)] = feed.events() else {
        panic!("one delete");
    };
    assert_eq!(1, deleted.identifiers().len());
}

#[test]
fn a_message_that_is_not_fhir_json_is_refused() {
    let body = message(&[update("p-1")]);
    assert_eq!(
        Err(FeedError::NotFhirJson),
        Feed::read(Some("application/fhir+xml"), body.as_bytes()).map(|_| ())
    );
    assert_eq!(
        Err(FeedError::NotFhirJson),
        Feed::read(None, body.as_bytes()).map(|_| ())
    );
}

#[test]
fn text_that_is_not_a_bundle_is_refused() {
    assert!(matches!(read("{not json"), Err(FeedError::NotJson { .. })));
    assert_eq!(Err(FeedError::NotAResource), read("[]").map(|_| ()));
    assert_eq!(
        Err(FeedError::UnexpectedResource),
        read(r#"{"resourceType":"Patient"}"#).map(|_| ())
    );
    assert!(matches!(
        read(r#"{"resourceType":"Bundle","type":"message","unknown":1}"#),
        Err(FeedError::Decode { .. })
    ));
}

#[test]
fn a_bundle_that_is_not_a_message_is_refused() {
    let body =
        message(&[update("p-1")]).replacen(r#""type":"message""#, r#""type":"collection""#, 1);
    assert_eq!(
        Err(FeedError::BundleType {
            expected: "message"
        }),
        read(&body).map(|_| ())
    );
}

#[test]
fn a_message_with_other_than_two_entries_is_refused() {
    let one = r#"{"resourceType":"Bundle","type":"message","entry":[{"fullUrl":"urn:uuid:7f4b3c2a-0d1c-4b0f-9a52-5d3c1c0e2f11"}]}"#;
    assert_eq!(
        Err(FeedError::EntryCount { found: 1 }),
        read(one).map(|_| ())
    );
}

#[test]
fn an_entry_without_a_full_url_is_refused() {
    let body = message(&[update("p-1")]).replacen(
        r#""fullUrl":"https://pmir.example.org/fhir/Bundle/h-1","#,
        "",
        1,
    );
    assert_eq!(
        Err(FeedError::NoFullUrl { entry: 1 }),
        read(&body).map(|_| ())
    );
}

#[test]
fn a_header_off_the_profile_is_refused() {
    let entries = [update("p-1")];
    let cases = [
        (
            HEADER.replacen(r#""id":"m-1","#, "", 1),
            FeedError::NoMessageId,
        ),
        (
            HEADER.replacen("patient-feed", "patient-feed-response", 1),
            FeedError::Event,
        ),
        (
            format!(
                r#"{HEADER},"definition":"https://profiles.ihe.net/ITI/PMIR/MessageDefinition/Other""#
            ),
            FeedError::Definition,
        ),
        (
            HEADER.replacen(
                r#""destination":[{"endpoint":"https://gateway.example.org/pmir/feed"}],"#,
                "",
                1,
            ),
            FeedError::NoDestination,
        ),
        (
            HEADER.replacen("Bundle/h-1", "Bundle/h-2", 1),
            FeedError::Focus,
        ),
        (
            HEADER.replacen(
                r#"[{"reference":"Bundle/h-1"}]"#,
                r#"[{"reference":"Bundle/h-1"},{"reference":"Bundle/h-1"}]"#,
                1,
            ),
            FeedError::Focus,
        ),
    ];
    for (header, refusal) in cases {
        assert_eq!(
            Err(refusal.clone()),
            read(&message_with(&header, &entries)).map(|_| ()),
            "{refusal}"
        );
    }
    let defined = format!(
        r#"{HEADER},"definition":"https://profiles.ihe.net/ITI/PMIR/MessageDefinition/IHE.PMIR.MessageDefinition""#
    );
    assert!(
        read(&message_with(&defined, &entries)).is_ok(),
        "the PMIR definition"
    );
}

#[test]
fn a_header_that_is_not_first_is_refused() {
    let body = message(&[update("p-1")]).replacen(
        r#""resourceType":"MessageHeader""#,
        r#""resourceType":"Basic","code":{"text":"x"}"#,
        1,
    );
    let body = body.replacen(r#","eventUri":"urn:ihe:iti:pmir:2019:patient-feed""#, "", 1);
    assert!(read(&body).is_err(), "no MessageHeader first");
}

#[test]
fn an_empty_history_is_refused() {
    // FHIR R4 json.html: an empty array is never written, so the history leaves
    // `entry` out, and one that writes it empty does not decode.
    let empty = message(&[]).replacen(r#","entry":[]"#, "", 1);
    assert_eq!(Err(FeedError::EmptyHistory), read(&empty).map(|_| ()));
    assert!(matches!(read(&message(&[])), Err(FeedError::Decode { .. })));
}

#[test]
fn a_history_entry_off_the_profile_is_refused() {
    let resource = patient("p-1", VALUE, "");
    let cases = [
        (
            entry("PATCH", "Patient/p-1", Some(&resource), "200"),
            FeedError::Method { index: 0 },
        ),
        (
            entry("PUT", "Patient/p-1", Some(&resource), "409 Conflict"),
            FeedError::Unsuccessful { index: 0 },
        ),
        (
            entry("PUT", "Patient/p-1", None, "200"),
            FeedError::NoResource { index: 0 },
        ),
        (
            entry(
                "PUT",
                "Patient/p-1",
                Some(r#"{"resourceType":"Basic","code":{"text":"x"}}"#),
                "200",
            ),
            FeedError::NotAPatient { index: 0 },
        ),
        (
            entry("PUT", "Patient/p-2", Some(&resource), "200"),
            FeedError::RequestUrl { index: 0 },
        ),
        (
            entry("DELETE", "Patient", None, "204"),
            FeedError::RequestUrl { index: 0 },
        ),
        (
            entry("POST", "Patient", Some(&merged("p-1", VALUE, "p-2")), "201"),
            FeedError::Merge { index: 0 },
        ),
    ];
    for (entry, refusal) in cases {
        assert_eq!(
            Err(refusal.clone()),
            read(&message(&[entry])).map(|_| ()),
            "{refusal}"
        );
    }
    let no_request =
        r#"{"fullUrl":"https://pmir.example.org/fhir/Patient/p-1","response":{"status":"200"}}"#;
    assert_eq!(
        Err(FeedError::NoRequest { index: 0 }),
        read(&message(&[no_request.to_owned()])).map(|_| ())
    );
    let no_response = r#"{"fullUrl":"https://pmir.example.org/fhir/Patient/p-1","request":{"method":"DELETE","url":"Patient/p-1"}}"#;
    assert_eq!(
        Err(FeedError::NoResponse { index: 0 }),
        read(&message(&[no_response.to_owned()])).map(|_| ())
    );
}

#[test]
fn a_merge_off_the_merged_patient_profile_is_refused() {
    let active = patient(
        "p-1",
        VALUE,
        r#","active":true,"link":[{"other":{"reference":"Patient/p-2"},"type":"replaced-by"}]"#,
    );
    let two_links = patient(
        "p-1",
        VALUE,
        r#","active":false,"link":[{"other":{"reference":"Patient/p-2"},"type":"replaced-by"},{"other":{"reference":"Patient/p-3"},"type":"seealso"}]"#,
    );
    let no_reference = patient(
        "p-1",
        VALUE,
        r#","active":false,"link":[{"other":{"display":"elsewhere"},"type":"replaced-by"}]"#,
    );
    for resource in [active, two_links, no_reference] {
        assert_eq!(
            Err(FeedError::Merge { index: 0 }),
            read(&message(&[entry(
                "PUT",
                "Patient/p-1",
                Some(&resource),
                "200"
            )]))
            .map(|_| ())
        );
    }
}

#[test]
fn two_entries_changing_one_patient_are_refused() {
    assert_eq!(
        Err(FeedError::Duplicate { index: 1 }),
        read(&message(&[update("p-1"), update("p-1")])).map(|_| ())
    );
}

#[test]
fn a_refusal_is_an_operation_outcome_of_the_errors_issue_type() {
    let error = FeedError::Merge { index: 2 };
    assert_eq!(IssueType::Invalid, error.issue());
    assert_eq!(IssueType::NotSupported, FeedError::NotFhirJson.issue());
    let body = String::from_utf8(refusal(&error).expect("an outcome")).expect("UTF-8");
    assert!(
        body.contains(r#""resourceType":"OperationOutcome""#),
        "{body}"
    );
    assert!(body.contains(r#""code":"invalid""#), "{body}");
}
