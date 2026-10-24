// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The operator API's writes over PostgreSQL 18 (#90), behind
//! `FERROPIX_E2E`: the reveal, a privacy disclosure recorded as a steward
//! action and a BALP disclosure record before the value is answered, and the
//! identifier domain writes behind the administrator scope, which the guard
//! takes at once.

use ferropix_operator_api::actions::Actions;
use ferropix_operator_api::domain::Domain;
use ferropix_operator_api::person::{IdentifierHandle, Person};
use ferropix_operator_api::reveal::{MAX_SECONDS, Revealed};
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::fixtures::Fixture;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN};
use http::{Method, StatusCode, header};

use super::BSN_SYSTEM;
use super::operator::{Fed, document, get, post};
use super::pixm::{Answer, Harness, LAB, fixture};
use crate::auth::bearer;

/// The profile every disclosure record claims.
const DISCLOSURE: &str =
    "https://profiles.ihe.net/ITI/BALP/StructureDefinition/IHE.BasicAudit.PrivacyDisclosure.Source";

/// Reads the person `fed` holds.
async fn person(fed: &Fed) -> Person {
    document(
        &get(&fed.harness, &format!("/operator/persons/{}", fed.person)).await,
        "person",
    )
}

/// Returns the JSON of a handle as a reveal names it.
fn target(handle: &IdentifierHandle) -> String {
    serde_json::to_string(handle).expect("a handle serializes")
}

/// Sends `method` on `uri` as `token` with the JSON `body`.
async fn send_as(harness: &Harness, method: Method, uri: &str, token: &str, body: &str) -> Answer {
    harness
        .send(
            method,
            uri,
            Some(token),
            &[(header::CONTENT_TYPE, "application/json")],
            Some(body.to_owned()),
        )
        .await
}

#[tokio::test]
async fn a_reveal_answers_the_value_once_after_its_action_and_its_disclosure_record() {
    if !e2e_enabled() {
        return;
    }
    let fed = Fed::start().await;
    let person = person(&fed).await;
    let hospital = person
        .identifiers
        .iter()
        .find(|row| row.domain.system == HOSPITAL_DOMAIN)
        .expect("the hospital identifier");
    let before = fed.harness.records();
    let answer = post(
        &fed.harness,
        &format!("/operator/persons/{}/reveal", fed.person),
        &format!(
            r#"{{"target":{},"reason":"the patient asked at the desk","seconds":600}}"#,
            target(&hospital.handle)
        ),
    )
    .await;
    let revealed: Revealed = document(&answer, "reveal");
    assert_eq!(fed.hospital, revealed.value.0);
    assert_eq!(HOSPITAL_DOMAIN, revealed.system);
    assert_eq!(MAX_SECONDS, revealed.seconds, "capped");
    assert_eq!(before + 1, fed.harness.records(), "one disclosure record");
    let disclosure = fed
        .harness
        .audit_texts()
        .into_iter()
        .find(|text| text.contains(DISCLOSURE))
        .expect("the disclosure record");
    assert!(disclosure.contains(&format!("Patient/{}", fed.person)));
    assert!(disclosure.contains("the patient asked at the desk"));
    assert!(disclosure.contains("\"disclose\""));
    assert!(
        !disclosure.contains(&fed.hospital),
        "the record names the domain, never the value"
    );
    let trail: Actions = document(
        &get(
            &fed.harness,
            &format!("/operator/actions?kind=reveal&person={}", fed.person),
        )
        .await,
        "the reveals",
    );
    assert_eq!(1, trail.actions.len());
    let action = &trail.actions[0];
    assert_eq!(revealed.action, action.id);
    assert_eq!("synthetic-subject-17", action.actor);
    assert_eq!(
        Some("the patient asked at the desk"),
        action.reason.as_deref()
    );
    assert!(action.domain.is_some(), "the action names the domain");
    assert!(
        !serde_json::to_string(action)
            .expect("JSON")
            .contains(&fed.hospital)
    );
}

#[tokio::test]
async fn a_carried_bsn_is_revealed_and_a_handle_the_person_does_not_hold_is_not_found() {
    if !e2e_enabled() {
        return;
    }
    let fed = Fed::start().await;
    let person = person(&fed).await;
    let carried = person
        .identifiers
        .iter()
        .find(|row| row.domain.system == BSN_SYSTEM)
        .expect("the carried BSN");
    let uri = format!("/operator/persons/{}/reveal", fed.person);
    let revealed: Revealed = document(
        &post(
            &fed.harness,
            &uri,
            &format!(
                r#"{{"target":{},"reason":"checking the BSN"}}"#,
                target(&carried.handle)
            ),
        )
        .await,
        "reveal of a carried BSN",
    );
    assert_eq!(fed.bsn, revealed.value.0);
    assert_eq!(15, revealed.seconds, "the default");
    let before = fed.harness.records();
    for body in [
        r#"{"target":{"record":"00000000-0000-7000-8000-000000000000"},"reason":"x"}"#,
        r#"{"target":{"carried":"not-a-uuid"},"reason":"x"}"#,
        r#"{"target":{"pseudonym":"00000000-0000-7000-8000-000000000000"},"reason":"x"}"#,
    ] {
        let answer = post(&fed.harness, &uri, body).await;
        assert_eq!(StatusCode::NOT_FOUND, answer.status, "{body}");
    }
    let other = post(
        &fed.harness,
        "/operator/persons/00000000-0000-7000-8000-000000000000/reveal",
        &format!(r#"{{"target":{},"reason":"x"}}"#, target(&carried.handle)),
    )
    .await;
    assert_eq!(
        StatusCode::NOT_FOUND,
        other.status,
        "another person's handle"
    );
    for reason in [r#""""#, r#""   ""#, &format!("\"{}\"", "x".repeat(501))] {
        let answer = post(
            &fed.harness,
            &uri,
            &format!(
                r#"{{"target":{},"reason":{reason}}}"#,
                target(&carried.handle)
            ),
        )
        .await;
        assert_eq!(StatusCode::BAD_REQUEST, answer.status, "{reason}");
    }
    assert_eq!(
        before,
        fed.harness.records(),
        "a refused reveal discloses nothing"
    );
    let got = get(&fed.harness, &uri).await;
    assert_eq!(StatusCode::METHOD_NOT_ALLOWED, got.status);
}

#[tokio::test]
async fn an_administrator_adds_a_domain_once_and_only_with_a_listed_source() {
    if !e2e_enabled() {
        return;
    }
    let fed = Fed::start().await;
    let admin = fed.harness.admin();
    let body = r#"{"system":"urn:oid:2.999.1.9","name":"Synthetic clinic","kind":"local","source_client_id":"west-lab","reason":"a new clinic"}"#;
    let created = send_as(
        &fed.harness,
        Method::POST,
        "/operator/domains",
        &admin,
        body,
    )
    .await;
    assert_eq!(StatusCode::CREATED, created.status, "{}", created.body);
    let domain: Domain = serde_json::from_str(&created.body).expect("a domain");
    assert_eq!("urn:oid:2.999.1.9", domain.system);
    assert!(domain.sensitive, "sealed unless said otherwise");
    let again = send_as(
        &fed.harness,
        Method::POST,
        "/operator/domains",
        &admin,
        body,
    )
    .await;
    assert_eq!(StatusCode::CONFLICT, again.status);
    let steward = send_as(
        &fed.harness,
        Method::POST,
        "/operator/domains",
        &fed.harness.steward(),
        body,
    )
    .await;
    assert_eq!(StatusCode::FORBIDDEN, steward.status);
    for (refused, status) in [
        (
            r#"{"system":"urn:oid:2.999.1.8","name":"X","kind":"local","source_client_id":"north-lab"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            r#"{"system":"urn:oid:2.999.1.8","name":"X","kind":"master","source_client_id":"west-lab"}"#,
            StatusCode::BAD_REQUEST,
        ),
        (
            r#"{"system":"not a uri","name":"X","kind":"local","source_client_id":"west-lab"}"#,
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let answer = send_as(
            &fed.harness,
            Method::POST,
            "/operator/domains",
            &admin,
            refused,
        )
        .await;
        assert_eq!(status, answer.status, "{refused}");
    }
    let trail: Actions = document(
        &get(&fed.harness, "/operator/actions?kind=domain-create").await,
        "the domain writes",
    );
    assert_eq!(1, trail.actions.len());
    assert_eq!(Some(domain.id.as_str()), trail.actions[0].domain.as_deref());
    assert_eq!(Some("a new clinic"), trail.actions[0].reason.as_deref());
}

#[tokio::test]
async fn a_new_source_feeds_the_moment_the_change_commits_and_a_retired_domain_stays_retired() {
    if !e2e_enabled() {
        return;
    }
    let fed = Fed::start().await;
    let admin = fed.harness.admin();
    let feeds = |client: &'static str| feed_laboratory(&fed.harness, client);
    assert_eq!(
        StatusCode::FORBIDDEN,
        feeds("west-lab").await.status,
        "not yet its Source"
    );
    let domains: ferropix_operator_api::domain::Domains =
        document(&get(&fed.harness, "/operator/domains").await, "domains");
    let laboratory = domains
        .domains
        .iter()
        .find(|domain| domain.system == LABORATORY_DOMAIN)
        .expect("the laboratory domain");
    let uri = format!("/operator/domains/{}", laboratory.id);
    let missing_reason = send_as(
        &fed.harness,
        Method::PATCH,
        &uri,
        &admin,
        r#"{"source_client_id":"west-lab","reason":" "}"#,
    )
    .await;
    assert_eq!(StatusCode::BAD_REQUEST, missing_reason.status);
    let changed = send_as(
        &fed.harness,
        Method::PATCH,
        &uri,
        &admin,
        r#"{"source_client_id":"west-lab","name":"Synthetic laboratory","reason":"the laboratory moved"}"#,
    )
    .await;
    let changed: Domain = document(&changed, "the changed domain");
    assert_eq!("west-lab", changed.source_client_id);
    assert_eq!("Synthetic laboratory", changed.name);
    assert!(
        feeds("west-lab").await.status.is_success(),
        "the new Source feeds"
    );
    assert_eq!(
        StatusCode::FORBIDDEN,
        feeds("east-lab").await.status,
        "the old one stops"
    );
    let retired: Domain = document(
        &send_as(
            &fed.harness,
            Method::PATCH,
            &uri,
            &admin,
            r#"{"retire":true,"reason":"the laboratory closed"}"#,
        )
        .await,
        "the retired domain",
    );
    assert!(retired.retired_at.is_some());
    let again = send_as(
        &fed.harness,
        Method::PATCH,
        &uri,
        &admin,
        r#"{"name":"Back","reason":"reopen"}"#,
    )
    .await;
    assert_eq!(StatusCode::CONFLICT, again.status);
    let unknown = send_as(
        &fed.harness,
        Method::PATCH,
        "/operator/domains/00000000-0000-7000-8000-000000000000",
        &admin,
        r#"{"name":"X","reason":"x"}"#,
    )
    .await;
    assert_eq!(StatusCode::NOT_FOUND, unknown.status);
    let trail: Actions = document(
        &get(&fed.harness, "/operator/actions?kind=domain-update").await,
        "the domain updates",
    );
    assert_eq!(2, trail.actions.len());
}

/// Feeds the laboratory's fixture as the client `client`, ITI-104.
async fn feed_laboratory(harness: &Harness, client: &str) -> Answer {
    harness
        .send(
            Method::PUT,
            &format!(
                "/fhir/Patient?{}",
                super::feeds::condition(LABORATORY_DOMAIN, LAB)
            ),
            Some(&bearer(&harness.issuer.token(client, "system/Patient.cud"))),
            &[(header::CONTENT_TYPE, "application/fhir+json")],
            Some(serde_json::to_string(&fixture(Fixture::FeedLaboratory)).expect("JSON")),
        )
        .await
}
