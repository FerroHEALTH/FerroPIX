// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-94 over PostgreSQL 18 (#105), behind `FERROPIX_E2E`: a subscription
//! created from the vendored PMIR example with each of the four criteria,
//! read, disabled, enabled and deleted; a fifth criteria, a missing channel
//! and an `http` endpoint refused; each request audited; the criteria
//! matching the persons a search finds; and the subscriptions in error
//! counted on the dashboard.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the Subscription bodies and answers are JSON values"
)]

use ferropix_operator_api::overview::Overview;
use ferropix_server::store::subscription;
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::HOSPITAL_DOMAIN;
use fhir_types::codec::Json as _;
use fhir_types::codec::Path;
use fhir_types::r4::patient::Patient;
use http::{Method, StatusCode, header};
use ihe_iti::pmir::server::subscription::Criteria;
use serde_json::Value;

use super::decisions::{fed, patient};
use super::operator::{document, get};
use super::pixm::{Answer, Harness, synthetic_seeds};
use crate::auth::bearer;

/// The FHIR JSON media type.
const FHIR_JSON: &str = "application/fhir+json";

/// The subscriber's token.
fn subscriber(harness: &Harness) -> String {
    bearer(
        &harness
            .issuer
            .token("synthetic-subscriber", "system/Subscription.cruds"),
    )
}

/// The vendored PMIR Subscription request, with `criteria` and an `https`
/// endpoint.
fn request(criteria: &str) -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/specs/ihe-pmir/package/example/Subscription-ex-subscription-request.json"
    );
    let mut value: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the example reads"))
            .expect("JSON");
    value["criteria"] = Value::String(criteria.to_owned());
    value["channel"]["endpoint"] = Value::String(String::from("https://consumer.example.org/pmir"));
    value
}

/// Sends `body` with `method` to `uri` as the subscriber.
async fn send(harness: &Harness, method: Method, uri: &str, body: Option<&Value>) -> Answer {
    let token = subscriber(harness);
    harness
        .send(
            method,
            uri,
            Some(&token),
            &[
                (header::CONTENT_TYPE, FHIR_JSON),
                (header::ACCEPT, FHIR_JSON),
            ],
            body.map(|body| serde_json::to_string(body).expect("JSON")),
        )
        .await
}

/// Creates a subscription with `criteria` and returns its id.
async fn created(harness: &Harness, criteria: &str) -> String {
    let answer = send(
        harness,
        Method::POST,
        "/fhir/Subscription",
        Some(&request(criteria)),
    )
    .await;
    assert_eq!(
        StatusCode::CREATED,
        answer.status,
        "{criteria}: {}",
        answer.body
    );
    let location = answer
        .headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a Location");
    let body: Value = serde_json::from_str(&answer.body).expect("JSON");
    let id = body["id"].as_str().expect("an id").to_owned();
    assert!(
        location.ends_with(&format!("/fhir/Subscription/{id}")),
        "{location}"
    );
    assert_eq!("active", body["status"], "activated at once");
    id
}

#[tokio::test]
async fn each_criteria_creates_a_subscription_and_anything_off_the_profile_is_refused() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let before = harness.records();
    for criteria in [
        "Patient",
        "Patient?_id=0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
        "Patient?organization=Organization/o1",
        "Patient?identifier=urn:oid:2.999.1.1%7C",
    ] {
        created(&harness, criteria).await;
    }
    let fifth = send(
        &harness,
        Method::POST,
        "/fhir/Subscription",
        Some(&request("Patient?gender=female")),
    )
    .await;
    assert_eq!(StatusCode::BAD_REQUEST, fifth.status, "{}", fifth.body);
    assert!(fifth.body.contains("not-supported"), "{}", fifth.body);
    let mut no_channel = request("Patient");
    no_channel
        .as_object_mut()
        .expect("an object")
        .remove("channel");
    let refused = send(
        &harness,
        Method::POST,
        "/fhir/Subscription",
        Some(&no_channel),
    )
    .await;
    assert_eq!(StatusCode::BAD_REQUEST, refused.status, "{}", refused.body);
    let mut plain = request("Patient");
    plain["channel"]["endpoint"] = Value::String(String::from("http://example.org/pmir-message"));
    let refused = send(&harness, Method::POST, "/fhir/Subscription", Some(&plain)).await;
    assert_eq!(StatusCode::BAD_REQUEST, refused.status, "{}", refused.body);
    assert!(refused.body.contains("not https"), "{}", refused.body);
    assert_eq!(before + 7, harness.records(), "one record per request");
    assert!(
        harness
            .audit_texts()
            .iter()
            .any(|text| text.contains("ITI-94")),
        "the records name ITI-94"
    );
}

#[tokio::test]
async fn a_subscription_is_read_disabled_enabled_and_deleted() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let id = created(&harness, "Patient").await;
    let uri = format!("/fhir/Subscription/{id}");
    let read = send(&harness, Method::GET, &uri, None).await;
    assert_eq!(StatusCode::OK, read.status, "{}", read.body);
    let mut off = request("Patient");
    off["status"] = Value::String(String::from("off"));
    let disabled = send(&harness, Method::PUT, &uri, Some(&off)).await;
    assert_eq!(StatusCode::OK, disabled.status, "{}", disabled.body);
    assert!(
        subscription::active(&harness.store)
            .await
            .expect("reads")
            .is_empty(),
        "a disabled subscription is sent nothing"
    );
    let enabled = send(&harness, Method::PUT, &uri, Some(&request("Patient"))).await;
    assert_eq!(StatusCode::OK, enabled.status, "{}", enabled.body);
    let body: Value = serde_json::from_str(&enabled.body).expect("JSON");
    assert_eq!("active", body["status"]);
    assert_eq!(
        1,
        subscription::active(&harness.store)
            .await
            .expect("reads")
            .len()
    );
    let mut error = request("Patient");
    error["status"] = Value::String(String::from("error"));
    let refused = send(&harness, Method::PUT, &uri, Some(&error)).await;
    assert_eq!(StatusCode::BAD_REQUEST, refused.status, "{}", refused.body);
    let deleted = send(&harness, Method::DELETE, &uri, None).await;
    assert_eq!(StatusCode::NO_CONTENT, deleted.status, "{}", deleted.body);
    assert_eq!(
        StatusCode::NOT_FOUND,
        send(&harness, Method::GET, &uri, None).await.status
    );
    assert_eq!(
        StatusCode::NOT_FOUND,
        send(&harness, Method::DELETE, &uri, None).await.status
    );
    assert_eq!(
        0,
        subscription::active(&harness.store)
            .await
            .expect("reads")
            .len()
    );
}

#[tokio::test]
async fn the_criteria_match_the_persons_a_search_finds() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    fed(
        &harness,
        HOSPITAL_DOMAIN,
        "H-1",
        &patient(HOSPITAL_DOMAIN, "H-1", "Wolderink", "Sanne", "1966-08-17"),
    )
    .await;
    let search = harness
        .send(
            Method::GET,
            "/fhir/Patient?family=Wolderink",
            Some(&harness.consumer()),
            &[(header::ACCEPT, FHIR_JSON)],
            None,
        )
        .await;
    let bundle: Value = serde_json::from_str(&search.body).expect("JSON");
    let served = bundle["entry"][0]["resource"]
        .as_object()
        .expect("a Patient")
        .clone();
    let person = served["id"].as_str().expect("an id").to_owned();
    let served: fhir_types::codec::Value =
        serde_json::from_value(Value::Object(served)).expect("a value");
    let served = Patient::from_json(
        served.as_object().expect("an object"),
        &mut Path::root("Patient"),
    )
    .expect("a Patient");
    for (criteria, want) in [
        (String::from("Patient"), true),
        (format!("Patient?_id={person}"), true),
        (
            String::from("Patient?_id=0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"),
            false,
        ),
        (
            String::from("Patient?identifier=urn:oid:2.999.1.1%7CH-1"),
            true,
        ),
        (
            String::from("Patient?identifier=urn:oid:2.999.1.1%7CH-2"),
            false,
        ),
        (String::from("Patient?organization=Organization/o1"), false),
    ] {
        assert_eq!(
            want,
            Criteria::parse(&criteria).expect("reads").matches(&served),
            "{criteria}"
        );
    }
}

#[tokio::test]
async fn the_subscriptions_in_error_are_counted_on_the_dashboard() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let failed = created(&harness, "Patient").await;
    created(&harness, "Patient").await;
    sqlx::query(
        "UPDATE pix.subscription SET status = 'error', error = 'unreachable' WHERE id = $1::uuid",
    )
    .bind(&failed)
    .execute(harness.store.pool())
    .await
    .expect("failed");
    let overview: Overview = document(&get(&harness, "/operator/overview").await, "the overview");
    assert_eq!(1, overview.subscriptions_in_error);
    let read = send(
        &harness,
        Method::GET,
        &format!("/fhir/Subscription/{failed}"),
        None,
    )
    .await;
    let body: Value = serde_json::from_str(&read.body).expect("JSON");
    assert_eq!("error", body["status"]);
    assert_eq!("unreachable", body["error"]);
    send(
        &harness,
        Method::DELETE,
        &format!("/fhir/Subscription/{failed}"),
        None,
    )
    .await;
    let overview: Overview = document(&get(&harness, "/operator/overview").await, "the overview");
    assert_eq!(
        0, overview.subscriptions_in_error,
        "a deleted subscription is no longer counted"
    );
}
