// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The PMIR 1.6.0 corpus: each example of the vendored package but the audit
//! records, read by the ITI-93 Consumer or the ITI-94 subscriber, or written
//! by them.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the vendored examples and the written resources are compared as JSON values"
)]

use ihe_iti::pmir::feed::{EventKind, Feed, ResponseId};
use ihe_iti::pmir::subscription::{Criteria, SubscriptionRequest, SubscriptionStatus};
use serde_json::Value;
use url::Url;

use super::{example, examples, is_audit, record, registry, verdict};
use crate::pmir::{FHIR_JSON, PROMPT, subscriber};

/// The vendored package under `docs/specs/`.
const PACKAGE: &str = "ihe-pmir";

/// The message the Consumer acknowledges in the response example.
const CREATE: &str = "example/Bundle-ex-PMIRBundleCreate.json";

/// The Consumer's channel endpoint.
const FEED_ENDPOINT: &str = "https://gateway.example.org/pmir/feed";

#[tokio::test]
async fn conformance_the_pmir_examples_hold_their_pass_list() {
    let mut cases = Vec::new();
    for id in examples(PACKAGE).into_iter().filter(|id| !is_audit(id)) {
        cases.push(verdict(id.clone(), check(id)).await);
    }
    record(
        "pmir",
        "The PMIR 1.6.0 examples of docs/specs/ihe-pmir, read and written through ihe-iti",
        cases,
    );
}

/// Checks the example `id` by the resource and the profile it holds.
async fn check(id: String) {
    let name = id.trim_start_matches("example/");
    let held = json(&id);
    let profile = held["meta"]["profile"][0].as_str().unwrap_or_default();
    if name.starts_with("Bundle-") {
        message(&id);
    } else if name.starts_with("MessageHeader-") {
        acknowledgement(&id);
    } else if name.starts_with("Subscription-") && profile.ends_with("Subscription.Request") {
        subscription_request(&id);
    } else if name.starts_with("Subscription-") {
        subscription(&id).await;
    } else {
        panic!("no ihe-iti reader or writer takes this example");
    }
}

fn json(id: &str) -> Value {
    serde_json::from_str(&example(PACKAGE, id)).expect("JSON")
}

/// The change an example message's name says it carries.
fn kind_named(id: &str) -> EventKind {
    for (word, kind) in [
        ("Create", EventKind::Create),
        ("Update", EventKind::Update),
        ("Delete", EventKind::Delete),
        ("Merge", EventKind::Merge),
    ] {
        if id.contains(word) {
            return kind;
        }
    }
    panic!("the message's name names no change");
}

/// An ITI-93 message, read by the Consumer as the changes it carries.
fn message(id: &str) {
    let feed = Feed::read(Some(FHIR_JSON), example(PACKAGE, id).as_bytes())
        .unwrap_or_else(|error| panic!("the Consumer refuses it: {error}"));
    let kind = kind_named(id);
    assert!(
        !feed.events().is_empty() && feed.count(kind) == feed.events().len(),
        "the Consumer reads its changes as {kind:?}: {:?}",
        feed.events()
    );
}

/// The response to an ITI-93 message, written by the Consumer for the create
/// message.
fn acknowledgement(id: &str) {
    let feed = Feed::read(Some(FHIR_JSON), example(PACKAGE, CREATE).as_bytes())
        .expect("the create message");
    let response_id = ResponseId::new("7f4b3c2a-0d1c-4b0f-9a52-5d3c1c0e2f11").expect("a UUID");
    let body = feed
        .acknowledgement(&response_id, FEED_ENDPOINT)
        .unwrap_or_else(|error| panic!("the Consumer cannot write it: {error}"));
    let written: Value = serde_json::from_slice(&body).expect("JSON");
    let header = &written["entry"][0]["resource"];
    let held = json(id);
    for key in ["resourceType", "eventUri", "response"] {
        assert_eq!(
            header[key], held[key],
            "the Consumer writes the {key} the example holds"
        );
    }
}

/// The ITI-94 subscription request, written by the subscriber for the
/// example's channel endpoint and criteria.
fn subscription_request(id: &str) {
    let held = json(id);
    assert_eq!(
        held["criteria"], "Patient",
        "the example subscribes to every Patient"
    );
    let endpoint = held["channel"]["endpoint"]
        .as_str()
        .expect("a channel endpoint");
    let request =
        SubscriptionRequest::new(Criteria::AllPatients, Url::parse(endpoint).expect("a URL"))
            .unwrap_or_else(|error| panic!("the subscriber cannot write it: {error}"));
    let written = serde_json::to_value(request.resource()).expect("JSON");
    for key in ["resourceType", "status", "criteria", "channel"] {
        assert_eq!(
            written[key], held[key],
            "the subscriber writes the {key} the example holds"
        );
    }
}

/// A Subscription the Registry holds, read by the subscriber for its status.
async fn subscription(id: &str) {
    let held = json(id);
    let server = registry(example(PACKAGE, id)).await;
    let client = subscriber(&server);
    let request = SubscriptionRequest::new(
        Criteria::AllPatients,
        Url::parse(FEED_ENDPOINT).expect("a URL"),
    )
    .expect("a request");
    let subscribed = client.subscribe(&request, PROMPT).await.expect("created");
    let status = client
        .status(&subscribed, PROMPT)
        .await
        .unwrap_or_else(|error| panic!("the subscriber refuses it: {error:?}"));
    let code = held["status"].as_str().expect("a status");
    let expected = match code {
        "requested" => SubscriptionStatus::Requested,
        "active" => SubscriptionStatus::Active,
        "error" => SubscriptionStatus::Error,
        "off" => SubscriptionStatus::Off,
        other => panic!("{other} is no subscription status"),
    };
    assert_eq!(status, expected, "the subscriber reads the status");
}
