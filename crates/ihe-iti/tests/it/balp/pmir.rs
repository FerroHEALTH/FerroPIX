// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-93 and ITI-94 audit records (PMIR 1.6.0 §2:3.93.5.1,
//! §2:3.94.5.1): each subscription create, read and delete of an audited
//! subscriber, held to the Subscription audit profiles and the Subscriber's
//! examples, and each ITI-93 message a Consumer received, held to the Feed
//! audit profile and the Consumer's example, naming the patients toward the
//! repository only.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the records and the vendored examples are read as JSON values"
)]

use std::sync::Arc;

use ihe_iti::balp::Outcome;
use ihe_iti::pmir::audit::received;
use ihe_iti::pmir::error::SubscribeError;
use ihe_iti::pmir::feed::Feed;
use ihe_iti::pmir::subscription::{Criteria, SubscriptionRequest};
use ihe_iti::user::OnBehalfOf;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::profile::{
    Kept, Refusing, base64_decoded, holds_to, like, names_no_user, vendored, written,
};
use crate::pmir::{DOMAIN, FHIR_JSON, PROMPT, subscriber};

const FEED_ENDPOINT: &str = "https://gateway.example.org/pmir/feed";

fn request() -> SubscriptionRequest {
    SubscriptionRequest::new(
        Criteria::identifier_system(DOMAIN).expect("an absolute URI"),
        Url::parse(FEED_ENDPOINT).expect("a URL"),
    )
    .expect("a request")
}

/// A Registry that creates `Subscription/s-1`, answers its read, and
/// deletes it.
async fn registry() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/fhir/Subscription"))
        .respond_with(ResponseTemplate::new(201).insert_header("location", "Subscription/s-1"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fhir/Subscription/s-1"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!(
                r#"{{"resourceType":"Subscription","id":"s-1","status":"active","reason":"x","criteria":"Patient","channel":{{"type":"message","endpoint":"{FEED_ENDPOINT}","payload":"application/fhir+json"}}}}"#
            ),
            FHIR_JSON,
        ))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/fhir/Subscription/s-1"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    server
}

/// A vendored file of the PMIR package.
fn pmir(file: &str) -> serde_json::Value {
    vendored("ihe-pmir", &format!("package/{file}"))
}

#[tokio::test]
async fn each_subscription_interaction_is_recorded_as_its_audit_profile_fixes_it() {
    let server = registry().await;
    let kept = Arc::new(Kept::default());
    let client = subscriber(&server).audited(kept.clone());
    let subscribed = client.subscribe(&request(), PROMPT).await.expect("created");
    client.status(&subscribed, PROMPT).await.expect("a status");
    client
        .unsubscribe(&subscribed, PROMPT)
        .await
        .expect("deleted");
    let [create, read, delete] = kept.taken().try_into().expect("three records");
    for (exchange, name) in [(&create, "Create"), (&read, "Read"), (&delete, "Delete")] {
        assert_eq!(exchange.outcome, Outcome::Success, "{name}");
        let record = written(exchange);
        holds_to(
            &record,
            &pmir(&format!(
                "StructureDefinition-IHE.PMIR.Audit.Subscription.{name}.json"
            )),
        );
        like(
            &record,
            &pmir(&format!(
                "example/AuditEvent-ex-auditPmirSubscription-subscriber-{}.json",
                name.to_lowercase()
            )),
            false,
        );
        assert_eq!(
            record["agent"][1]["who"]["display"],
            format!("{}/fhir/", server.uri()),
            "{name}: the Registry is named by its base"
        );
        let data = &record["entity"][0];
        assert_eq!(data["what"]["reference"], "Subscription/s-1", "{name}");
        assert_eq!(data["what"]["type"], "Subscription", "{name}");
        assert!(
            record["entity"]
                .as_array()
                .expect("entities")
                .iter()
                .all(|entity| entity["role"]["code"] != "1"),
            "a subscription names no patient"
        );
    }
    let criteria = base64_decoded(
        written(&create)["entity"][0]["query"]
            .as_str()
            .expect("the criteria"),
    );
    assert_eq!(criteria, format!("Patient?identifier={DOMAIN}|"));
}

#[tokio::test]
async fn a_refused_create_is_recorded_without_a_subscription_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;
    let kept = Arc::new(Kept::default());
    let client = subscriber(&server).audited(kept.clone());
    client
        .subscribe(&request(), PROMPT)
        .await
        .expect_err("a refusal");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::MinorFailure);
    let record = written(&exchange);
    assert_eq!(record["entity"][0]["what"]["type"], "Subscription");
    assert!(record["entity"][0]["what"].get("reference").is_none());
}

#[tokio::test]
async fn the_search_by_channel_endpoint_is_recorded_on_the_balp_query_pattern() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/fhir/Subscription"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let kept = Arc::new(Kept::default());
    let client = subscriber(&server).audited(kept.clone());
    client.find(&request(), PROMPT).await.expect("an answer");
    let [exchange] = kept.taken().try_into().expect("one record");
    let record = written(&exchange);
    holds_to(
        &record,
        &vendored(
            "ihe-balp",
            "package/StructureDefinition-IHE.BasicAudit.Query.json",
        ),
    );
    like(
        &record,
        &vendored(
            "ihe-balp",
            "package/example/AuditEvent-ex-auditBasicQueryGetClient.json",
        ),
        false,
    );
}

#[tokio::test]
async fn an_exchange_whose_record_is_refused_fails() {
    let server = registry().await;
    let client = subscriber(&server).audited(Arc::new(Refusing));
    let error = client
        .subscribe(&request(), PROMPT)
        .await
        .expect_err("the create fails closed");
    assert!(matches!(error, SubscribeError::Audit(_)), "{error:?}");
    assert!(!error.answered(), "an answer set aside is no answer");
}

/// The Consumer's record of the vendored create message.
fn create_record() -> (serde_json::Value, String) {
    let body = crate::pmir::vendored("example/Bundle-ex-PMIRBundleCreate.json");
    let feed = Feed::read(Some(FHIR_JSON), body.as_bytes()).expect("the example message");
    let exchange = received(
        &Url::parse("https://pmir.example.org/fhir/").expect("a URL"),
        &Url::parse(FEED_ENDPOINT).expect("a URL"),
        Some(&feed),
    );
    assert_eq!(exchange.outcome, Outcome::Success);
    (written(&exchange), format!("{exchange:?}"))
}

#[tokio::test]
async fn every_subscription_interaction_and_received_message_names_no_user() {
    let server = registry().await;
    let kept = Arc::new(Kept::default());
    let client = subscriber(&server).audited(kept.clone());
    let subscribed = client.subscribe(&request(), PROMPT).await.expect("created");
    client.status(&subscribed, PROMPT).await.expect("a status");
    client
        .unsubscribe(&subscribed, PROMPT)
        .await
        .expect("deleted");
    for exchange in kept.taken() {
        assert_eq!(exchange.on_behalf, OnBehalfOf::System);
        names_no_user(&written(&exchange));
    }
    let (record, _) = create_record();
    names_no_user(&record);
}

#[test]
fn a_received_message_is_recorded_as_the_feed_audit_profile_fixes_it() {
    let (record, _) = create_record();
    holds_to(
        &record,
        &pmir("StructureDefinition-IHE.PMIR.Feed.Audit.json"),
    );
    like(
        &record,
        &pmir("example/AuditEvent-ex-auditPmirFeed-consumer.json"),
        true,
    );
    assert_eq!(
        record["agent"][1]["network"]["address"], FEED_ENDPOINT,
        "the Consumer is the destination, at its channel endpoint"
    );
}

#[test]
fn the_received_record_names_each_patient_toward_the_repository_only() {
    let (record, debug) = create_record();
    let example = pmir("example/AuditEvent-ex-auditPmirFeed-consumer.json");
    let patients: Vec<&serde_json::Value> = record["entity"]
        .as_array()
        .expect("entities")
        .iter()
        .filter(|entity| entity["role"]["code"] == "1")
        .collect();
    assert!(!patients.is_empty(), "a create message names its patient");
    let named = example["entity"][0]["what"]["reference"]
        .as_str()
        .expect("the example's patient");
    assert!(
        patients
            .iter()
            .any(|patient| patient["what"]["reference"] == named),
        "the record names {named}, as the example does"
    );
    assert!(!debug.contains(named), "Debug names no patient");
    let message = record["entity"]
        .as_array()
        .expect("entities")
        .iter()
        .find(|entity| entity["type"]["code"] == "MessageHeader")
        .expect("the message entity");
    assert!(
        message["what"]["reference"]
            .as_str()
            .is_some_and(|reference| reference.starts_with("MessageHeader/"))
    );
}

#[test]
fn a_refused_message_is_recorded_as_a_minor_failure_naming_no_patient() {
    let exchange = received(
        &Url::parse("https://pmir.example.org/fhir/").expect("a URL"),
        &Url::parse(FEED_ENDPOINT).expect("a URL"),
        None,
    );
    assert_eq!(exchange.outcome, Outcome::MinorFailure);
    assert!(exchange.entities.is_empty());
}
