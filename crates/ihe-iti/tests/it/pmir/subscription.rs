// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-94 against a stub Patient Identity Registry: the create and its
//! `Location` (§2:3.94.4.1, §2:3.94.4.2), the read of the status
//! (§2:3.94.4.3), the delete (§2:3.94.4.5), and every answer that is not one
//! of those.

use fhir_types::codec::{Object, Value};
use http::StatusCode;
use ihe_iti::outcome::IssueType;
use ihe_iti::pmir::PmirSubscriber;
use ihe_iti::pmir::error::{SubscribeError, SubscriptionMalformation};
use ihe_iti::pmir::subscription::{Criteria, Search, SubscriptionRequest, SubscriptionStatus};
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{DOMAIN, FHIR_JSON, PROMPT, subscriber};

fn request() -> SubscriptionRequest {
    SubscriptionRequest::new(
        Criteria::identifier_system(DOMAIN).expect("an absolute URI"),
        Url::parse("https://gateway.example.org/pmir/feed").expect("a URL"),
    )
    .expect("a request")
}

async fn registry_creating(location: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/fhir/Subscription"))
        .and(header("content-type", FHIR_JSON))
        .respond_with(ResponseTemplate::new(201).insert_header("location", location))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn a_created_subscription_is_read_back_from_its_location() {
    let server = registry_creating("Subscription/s-1/_history/1").await;
    let subscribed = subscriber(&server)
        .subscribe(&request(), PROMPT)
        .await
        .expect("created");
    assert_eq!(
        format!("{}/fhir/Subscription/s-1", server.uri()),
        subscribed.location().as_str()
    );
    let sent = server.received_requests().await.expect("recorded");
    let body: Object =
        serde_json::from_slice(&sent.first().expect("one request").body).expect("a JSON object");
    assert_eq!(
        Some("Subscription"),
        body.get("resourceType").and_then(Value::as_str)
    );
    assert_eq!(
        Some("Patient?identifier=urn:oid:2.999.1.47|"),
        body.get("criteria").and_then(Value::as_str),
        "limited by system (§2:3.94.4.1.2.1.1)"
    );
    let channel = body.get("channel").expect("a channel");
    assert_eq!(
        Some("https://gateway.example.org/pmir/feed"),
        channel.get("endpoint").and_then(Value::as_str)
    );
    assert_eq!(
        Some(FHIR_JSON),
        channel.get("payload").and_then(Value::as_str)
    );
}

#[tokio::test]
async fn a_created_subscription_without_a_location_is_malformed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;
    let error = subscriber(&server)
        .subscribe(&request(), PROMPT)
        .await
        .expect_err("no Location");
    assert!(matches!(
        error,
        SubscribeError::Malformed(SubscriptionMalformation::NoLocation)
    ));
}

#[tokio::test]
async fn a_location_off_the_base_is_never_followed() {
    let server = registry_creating("https://elsewhere.example.org/fhir/Subscription/s-1").await;
    let error = subscriber(&server)
        .subscribe(&request(), PROMPT)
        .await
        .expect_err("elsewhere");
    assert!(matches!(
        error,
        SubscribeError::Malformed(SubscriptionMalformation::Location)
    ));
}

#[tokio::test]
async fn a_refused_subscription_keeps_the_status_and_the_issue_codes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(403).set_body_raw(
            r#"{"resourceType":"OperationOutcome","issue":[{"severity":"error","code":"forbidden"}]}"#,
            FHIR_JSON,
        ))
        .mount(&server)
        .await;
    let error = subscriber(&server)
        .subscribe(&request(), PROMPT)
        .await
        .expect_err("refused");
    assert_eq!(Some(StatusCode::FORBIDDEN), error.status());
    assert!(error.answered());
    let SubscribeError::Rejected { issues, .. } = error else {
        panic!("a rejection");
    };
    assert_eq!(vec![IssueType::Forbidden], issues);
}

#[tokio::test]
async fn an_unreachable_registry_is_a_transport_failure() {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("an HTTP client");
    let base = Url::parse("http://127.0.0.1:0/fhir/").expect("a base");
    let error = PmirSubscriber::new(base, http)
        .expect("a subscriber")
        .subscribe(&request(), PROMPT)
        .await
        .expect_err("nothing listens");
    assert!(matches!(error, SubscribeError::Transport(_)), "{error:?}");
    assert!(!error.answered());
}

#[tokio::test]
async fn the_status_is_read_from_the_subscription() {
    let server = registry_creating("Subscription/s-1").await;
    Mock::given(method("GET"))
        .and(path("/fhir/Subscription/s-1"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"resourceType":"Subscription","id":"s-1","status":"error","reason":"x","criteria":"Patient","channel":{"type":"message","endpoint":"https://gateway.example.org/pmir/feed","payload":"application/fhir+json"}}"#,
            FHIR_JSON,
        ))
        .mount(&server)
        .await;
    let client = subscriber(&server);
    let subscribed = client.subscribe(&request(), PROMPT).await.expect("created");
    assert_eq!(
        SubscriptionStatus::Error,
        client.status(&subscribed, PROMPT).await.expect("a status")
    );
}

#[tokio::test]
async fn a_status_outside_the_value_set_is_malformed() {
    let server = registry_creating("Subscription/s-1").await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"resourceType":"Subscription","status":"paused","reason":"x","criteria":"Patient","channel":{"type":"message"}}"#,
            FHIR_JSON,
        ))
        .mount(&server)
        .await;
    let client = subscriber(&server);
    let subscribed = client.subscribe(&request(), PROMPT).await.expect("created");
    let error = client
        .status(&subscribed, PROMPT)
        .await
        .expect_err("paused");
    assert!(matches!(
        error,
        SubscribeError::Malformed(SubscriptionMalformation::Status)
    ));
}

#[tokio::test]
async fn a_delete_of_a_gone_subscription_is_a_deletion() {
    for status in [204, 404, 410] {
        let server = registry_creating("Subscription/s-1").await;
        Mock::given(method("DELETE"))
            .and(path("/fhir/Subscription/s-1"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;
        let client = subscriber(&server);
        let subscribed = client.subscribe(&request(), PROMPT).await.expect("created");
        client
            .unsubscribe(&subscribed, PROMPT)
            .await
            .unwrap_or_else(|error| panic!("{status}: {error}"));
    }
    let server = registry_creating("Subscription/s-1").await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let client = subscriber(&server);
    let subscribed = client.subscribe(&request(), PROMPT).await.expect("created");
    let error = client
        .unsubscribe(&subscribed, PROMPT)
        .await
        .expect_err("500");
    assert_eq!(Some(StatusCode::INTERNAL_SERVER_ERROR), error.status());
}

/// A searchset answer listing `subscriptions`, each `(id, criteria, endpoint,
/// status)`.
fn searchset(subscriptions: &[(&str, &str, &str, &str)]) -> String {
    let entries: Vec<String> = subscriptions
        .iter()
        .map(|(id, criteria, endpoint, status)| {
            format!(
                r#"{{"resource":{{"resourceType":"Subscription","id":"{id}","status":"{status}","reason":"x","criteria":"{criteria}","channel":{{"type":"message","endpoint":"{endpoint}","payload":"application/fhir+json"}}}}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"resourceType":"Bundle","type":"searchset","entry":[{}]}}"#,
        entries.join(",")
    )
}

#[tokio::test]
async fn a_search_lists_only_the_requests_own_subscriptions() {
    let server = MockServer::start().await;
    let criteria = format!("Patient?identifier={DOMAIN}|");
    Mock::given(method("GET"))
        .and(path("/fhir/Subscription"))
        .and(wiremock::matchers::query_param(
            "url",
            "https://gateway.example.org/pmir/feed",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            searchset(&[
                (
                    "other",
                    "Patient",
                    "https://gateway.example.org/pmir/feed",
                    "active",
                ),
                (
                    "elsewhere",
                    &criteria,
                    "https://other.example.org/feed",
                    "active",
                ),
                (
                    "s-1",
                    &criteria,
                    "https://gateway.example.org/pmir/feed",
                    "off",
                ),
            ]),
            FHIR_JSON,
        ))
        .mount(&server)
        .await;
    let Search::Found(found) = subscriber(&server)
        .find(&request(), PROMPT)
        .await
        .expect("a search")
    else {
        panic!("the search is supported");
    };
    let [listed] = found.as_slice() else {
        panic!("one of the three is the request's: {found:?}");
    };
    assert_eq!(SubscriptionStatus::Off, listed.status);
    assert_eq!(
        format!("{}/fhir/Subscription/s-1", server.uri()),
        listed.subscribed.location().as_str()
    );
}

#[tokio::test]
async fn a_registry_that_cannot_search_by_url_is_unsupported() {
    for status in [400, 404] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;
        assert_eq!(
            Search::Unsupported,
            subscriber(&server)
                .find(&request(), PROMPT)
                .await
                .expect("an answer"),
            "{status}"
        );
    }
}

#[tokio::test]
async fn a_listed_subscription_without_an_id_is_malformed() {
    let server = MockServer::start().await;
    let body = searchset(&[(
        "",
        &format!("Patient?identifier={DOMAIN}|"),
        "https://gateway.example.org/pmir/feed",
        "active",
    )])
    .replacen(r#""id":"","#, "", 1);
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, FHIR_JSON))
        .mount(&server)
        .await;
    let error = subscriber(&server)
        .find(&request(), PROMPT)
        .await
        .expect_err("no id");
    assert!(matches!(
        error,
        SubscribeError::Malformed(SubscriptionMalformation::NoId)
    ));
}
