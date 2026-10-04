// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The feed and the subscription held to the vendored PMIR 1.6.0 artefacts:
//! the IG's four message Bundle examples read as the changes they show
//! (§2:3.93.4.1.2.6), the response held to the `MessageHeader` response
//! profile and its example, and the subscription request held to the
//! `Subscription` request profile and its example.

use fhir_types::codec::{Object, Value};
use ihe_iti::pmir::feed::{Event, EventKind, Feed, ResponseId};
use ihe_iti::pmir::subscription::{Criteria, SubscriptionRequest};
use secrecy::ExposeSecret;
use serde::Deserialize;
use url::Url;

use super::{FHIR_JSON, vendored};

fn example(name: &str) -> Feed {
    Feed::read(
        Some(FHIR_JSON),
        vendored(&format!("example/{name}")).as_bytes(),
    )
    .unwrap_or_else(|error| panic!("{name} reads: {error}"))
}

#[test]
fn the_merge_example_is_one_merge_into_the_surviving_patient() {
    let feed = example("Bundle-ex-PMIRBundleMerge.json");
    assert_eq!("ex-messageheader-merge", feed.message_id());
    let [
        Event::Merged {
            subsumed,
            surviving,
        },
    ] = feed.events()
    else {
        panic!("one merge: {:?}", feed.events());
    };
    assert_eq!(
        Some("ex-patient-merge"),
        subsumed.id().map(ExposeSecret::expose_secret)
    );
    assert_eq!("Patient/ex-patient-merged", surviving.expose_secret());
    assert!(
        subsumed.identifiers().is_empty(),
        "the example's merged Patient carries no identifier"
    );
}

#[test]
fn the_create_example_is_two_creates() {
    let feed = example("Bundle-ex-PMIRBundleCreate.json");
    assert_eq!(2, feed.count(EventKind::Create));
    assert_eq!(2, feed.events().len());
}

#[test]
fn the_update_example_is_one_update() {
    let feed = example("Bundle-ex-PMIRBundleUpdate.json");
    assert_eq!(1, feed.count(EventKind::Update));
    assert_eq!(1, feed.events().len());
}

#[test]
fn the_delete_example_is_one_delete_named_by_its_request() {
    let feed = example("Bundle-ex-PMIRBundleDelete.json");
    let [Event::Deleted(deleted)] = feed.events() else {
        panic!("one delete: {:?}", feed.events());
    };
    assert_eq!(
        Some("ex-patient-delete"),
        deleted.id().map(ExposeSecret::expose_secret)
    );
}

/// What the tests read of the response message.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    resource_type: String,
    r#type: String,
    entry: Vec<ResponseEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResponseEntry {
    full_url: String,
    resource: Header,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    resource_type: String,
    id: Option<String>,
    event_uri: String,
    destination: Option<Vec<serde::de::IgnoredAny>>,
    source: Source,
    response: Answer,
    definition: Option<String>,
}

#[derive(Deserialize)]
struct Source {
    endpoint: String,
}

#[derive(Deserialize)]
struct Answer {
    identifier: String,
    code: String,
}

#[test]
fn the_acknowledgement_holds_to_the_response_profile() {
    let feed = example("Bundle-ex-PMIRBundleMerge.json");
    let id = ResponseId::new("7f4b3c2a-0d1c-4b0f-9a52-5d3c1c0e2f11").expect("a UUID");
    let body = feed
        .acknowledgement(&id, "https://gateway.example.org/pmir/feed")
        .expect("a response");
    let response: Response = serde_json::from_slice(&body).expect("JSON");
    assert_eq!(
        ("Bundle", "message"),
        (response.resource_type.as_str(), response.r#type.as_str())
    );
    let [entry] = response.entry.as_slice() else {
        panic!("one entry (§2:3.93.4.2.2)");
    };
    assert_eq!(
        "urn:uuid:7f4b3c2a-0d1c-4b0f-9a52-5d3c1c0e2f11",
        entry.full_url
    );
    let header = &entry.resource;
    assert_eq!("MessageHeader", header.resource_type);
    assert_eq!(Some(id.as_str()), header.id.as_deref());
    let profile: Profile = serde_json::from_str(&vendored(
        "StructureDefinition-IHE.PMIR.MessageHeader.Response.json",
    ))
    .expect("the profile");
    assert_eq!(
        profile.pattern("MessageHeader.event[x]", "patternUri"),
        Some(header.event_uri.clone())
    );
    assert_eq!(
        profile.pattern("MessageHeader.definition", "patternCanonical"),
        header.definition.clone()
    );
    assert!(header.destination.is_none(), "destination is 0..0");
    assert_eq!("ex-messageheader-merge", header.response.identifier);
    assert_eq!("ok", header.response.code);
    assert_eq!(
        "https://gateway.example.org/pmir/feed",
        header.source.endpoint
    );
    let example: Header = serde_json::from_str(&vendored(
        "example/MessageHeader-ex-messageheader-create-response.json",
    ))
    .expect("the example");
    assert_eq!(example.event_uri, header.event_uri, "as the IG's example");
}

#[test]
fn a_response_id_is_a_lowercase_hyphenated_uuid() {
    assert!(ResponseId::new("7f4b3c2a-0d1c-4b0f-9a52-5d3c1c0e2f11").is_ok());
    for refused in [
        "7F4B3C2A-0D1C-4B0F-9A52-5D3C1C0E2F11",
        "7f4b3c2a0d1c4b0f9a525d3c1c0e2f11",
        "7f4b3c2a-0d1c-4b0f-9a52-5d3c1c0e2f1",
        "",
    ] {
        assert!(ResponseId::new(refused).is_err(), "{refused}");
    }
}

/// The differential of a `StructureDefinition`, read for its fixed patterns.
#[derive(Deserialize)]
struct Profile {
    differential: Differential,
}

#[derive(Deserialize)]
struct Differential {
    element: Vec<Object>,
}

impl Profile {
    /// The string `key` of the element `id`, when the differential fixes one.
    fn pattern(&self, id: &str, key: &str) -> Option<String> {
        self.differential
            .element
            .iter()
            .find(|element| element.get("id").and_then(Value::as_str) == Some(id))
            .and_then(|element| element.get(key))
            .and_then(Value::as_str)
            .map(str::to_owned)
    }
}

#[test]
fn the_subscription_request_holds_to_the_request_profile() {
    let request = SubscriptionRequest::new(
        Criteria::AllPatients,
        Url::parse("http://example.org/pmir-message").expect("a URL"),
    )
    .expect("a request");
    let written: Object =
        serde_json::from_slice(&serde_json::to_vec(&request.resource()).expect("JSON"))
            .expect("an object");
    let mut example: Object = serde_json::from_str(&vendored(
        "example/Subscription-ex-subscription-request.json",
    ))
    .expect("the example");
    let request_profile: Profile = serde_json::from_str(&vendored(
        "StructureDefinition-IHE.PMIR.Subscription.Request.json",
    ))
    .expect("the request profile");
    let profile: Profile =
        serde_json::from_str(&vendored("StructureDefinition-IHE.PMIR.Subscription.json"))
            .expect("the profile");
    assert_eq!(
        request_profile
            .pattern("Subscription.status", "patternCode")
            .as_deref(),
        written.get("status").and_then(Value::as_str)
    );
    let channel = written.get("channel").expect("a channel");
    assert_eq!(
        profile
            .pattern("Subscription.channel.type", "patternCode")
            .as_deref(),
        channel.get("type").and_then(Value::as_str)
    );
    assert!(
        channel.get("header").is_none(),
        "no credential in the subscription (§2:3.94.5)"
    );
    for key in ["criteria", "channel"] {
        assert_eq!(
            example.get(key),
            written.get(key),
            "{key} as the IG's example"
        );
    }
    example.retain(|key, _| {
        key != "id" && key != "meta" && key != "contact" && key != "reason" && key != "text"
    });
    for key in example.keys() {
        assert!(
            written.contains_key(key),
            "{key} is written as the example writes it"
        );
    }
}
