// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The operator API over PostgreSQL 18 (#90), behind `FERROPIX_E2E`: every
//! served route's document read from a store the PIXm feed filled, each
//! answered `no-store`, with no identifier value in any answer, and the
//! steward actions and BALP records a view, a search and an export write.

use ferropix_operator_api::actions::{Actions, EXPORT_NEXT};
use ferropix_operator_api::domain::{DomainPage, Domains};
use ferropix_operator_api::overview::{FEED_HOURS, Overview, QUEUE_DAYS};
use ferropix_operator_api::patient::PersonStatus;
use ferropix_operator_api::person::{IdentifierHandle, Person, RecordState};
use ferropix_operator_api::search::Results;
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN};
use http::{Method, StatusCode, header};

use super::BSN_SYSTEM;
use super::pixm::{Answer, Harness, LAB, feed_both, synthetic_seeds};

/// Asserts `answer` is a `200` no cache keeps and reads its body as `T`.
pub(crate) fn document<T: serde::de::DeserializeOwned>(answer: &Answer, label: &str) -> T {
    assert_eq!(StatusCode::OK, answer.status, "{label}: {}", answer.body);
    assert_eq!(
        Some("no-store"),
        answer
            .headers
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        "{label}"
    );
    serde_json::from_str(&answer.body).unwrap_or_else(|_| panic!("{label}: {}", answer.body))
}

/// `GET uri` as the steward.
pub(crate) async fn get(harness: &Harness, uri: &str) -> Answer {
    harness
        .send(Method::GET, uri, Some(&harness.steward()), &[], None)
        .await
}

/// `POST uri` of the JSON `body` as the steward.
pub(crate) async fn post(harness: &Harness, uri: &str, body: &str) -> Answer {
    harness
        .send(
            Method::POST,
            uri,
            Some(&harness.steward()),
            &[(header::CONTENT_TYPE, "application/json")],
            Some(body.to_owned()),
        )
        .await
}

/// Asserts no identifier value of the fed patients is in `text`.
fn holds_no_identifier(text: &str, values: &[&str], label: &str) {
    for value in values {
        assert!(!text.contains(value), "{label} holds an identifier: {text}");
    }
}

/// A Manager fed the hospital and the laboratory, joined by the BSN, with
/// the id of their one person.
pub(crate) struct Fed {
    pub(crate) harness: Harness,
    pub(crate) hospital: String,
    pub(crate) bsn: String,
    pub(crate) person: String,
}

impl Fed {
    /// Starts the Manager, feeds both records and finds their person.
    pub(crate) async fn start() -> Self {
        let harness = Harness::start(synthetic_seeds()).await;
        let (hospital, bsn) = feed_both(&harness).await;
        let found: Results = document(
            &post(
                &harness,
                "/operator/search/identifier",
                &format!(r#"{{"system":"{HOSPITAL_DOMAIN}","value":"{hospital}"}}"#),
            )
            .await,
            "search by identifier",
        );
        let person = found.persons[0].id.clone();
        Self {
            harness,
            hospital,
            bsn,
            person,
        }
    }

    /// Asserts no fed identifier value is in `text`.
    pub(crate) fn holds_no_identifier(&self, text: &str, label: &str) {
        holds_no_identifier(text, &[&self.hospital, &self.bsn, LAB], label);
    }
}

#[tokio::test]
async fn the_overview_counts_the_records_the_links_and_the_feeds_of_the_store() {
    if !e2e_enabled() {
        return;
    }
    let fed = Fed::start().await;
    let overview: Overview = document(&get(&fed.harness, "/operator/overview").await, "overview");
    assert_eq!(2, overview.records_created_24h);
    assert_eq!(
        2, overview.records_linked_24h,
        "the BSN joined the two records"
    );
    assert_eq!(2, overview.versions_fed_today);
    assert_eq!(QUEUE_DAYS, overview.queue_depth.len());
    assert_eq!(FEED_HOURS, overview.feeds_per_hour.len());
    let fhir: u64 = overview.feeds_per_hour.iter().map(|hour| hour.fhir).sum();
    assert_eq!(2, fhir);
    assert_eq!(0, overview.queue.open);
}

#[tokio::test]
async fn an_identifier_search_finds_the_person_by_its_own_or_a_carried_identifier_masked() {
    if !e2e_enabled() {
        return;
    }
    let fed = Fed::start().await;
    let before = fed.harness.records();
    let answer = post(
        &fed.harness,
        "/operator/search/identifier",
        &format!(r#"{{"system":"{BSN_SYSTEM}","value":"{}"}}"#, fed.bsn),
    )
    .await;
    fed.holds_no_identifier(&answer.body, "an identifier search");
    let found: Results = document(&answer, "search by a carried BSN");
    assert_eq!(1, found.total);
    assert_eq!(fed.person, found.persons[0].id);
    assert_eq!(
        3,
        found.persons[0].identifiers.len(),
        "hospital, laboratory, BSN"
    );
    assert!(found.persons[0].score.is_none());
    assert_eq!(
        before + 1,
        fed.harness.records(),
        "one Patient Query record"
    );
    let none: Results = document(
        &post(
            &fed.harness,
            "/operator/search/identifier",
            &format!(r#"{{"system":"{HOSPITAL_DOMAIN}","value":"H-999999"}}"#),
        )
        .await,
        "search by an unknown identifier",
    );
    assert_eq!((0, 0), (none.total, none.persons.len()));
    assert_eq!(before + 2, fed.harness.records(), "one Query record");
    let unknown = post(
        &fed.harness,
        "/operator/search/identifier",
        r#"{"system":"urn:oid:2.999.9.9","value":"x"}"#,
    )
    .await;
    assert_eq!(StatusCode::UNPROCESSABLE_ENTITY, unknown.status);
}

#[tokio::test]
async fn a_demographics_search_scores_and_grades_the_person() {
    if !e2e_enabled() {
        return;
    }
    let fed = Fed::start().await;
    let answer = post(
        &fed.harness,
        "/operator/search/demographics",
        r#"{"family":"Proefsma","birth_date":"1961-04-12","postal_code":"0000 AA"}"#,
    )
    .await;
    fed.holds_no_identifier(&answer.body, "a demographics search");
    let scored: Results = document(&answer, "search by demographics");
    assert_eq!(1, scored.total);
    assert_eq!(fed.person, scored.persons[0].id);
    assert!(scored.persons[0].score.is_some_and(|score| score > 0.0));
    assert!(scored.persons[0].grade.is_some());
}

#[tokio::test]
async fn the_person_shows_its_identifiers_masked_its_records_and_history_and_records_the_view() {
    if !e2e_enabled() {
        return;
    }
    let fed = Fed::start().await;
    let before = fed.harness.records();
    let answer = get(&fed.harness, &format!("/operator/persons/{}", fed.person)).await;
    fed.holds_no_identifier(&answer.body, "the person");
    let person: Person = document(&answer, "person");
    assert_eq!(PersonStatus::Active, person.status);
    assert_eq!(3, person.identifiers.len());
    assert_eq!(2, person.records.len());
    assert!(
        person.identifiers.iter().any(|row| {
            matches!(row.handle, IdentifierHandle::Carried(_)) && row.domain.system == BSN_SYSTEM
        }),
        "the BSN no Source feeds is carried"
    );
    assert!(
        person
            .records
            .iter()
            .all(|r| r.state == RecordState::Active)
    );
    assert!(
        person
            .records
            .iter()
            .any(|record| record.domain.system == LABORATORY_DOMAIN)
    );
    assert_eq!(
        Some("Proefsma"),
        person
            .leading
            .as_ref()
            .and_then(|leading| leading.names.first())
            .and_then(|name| name.family.as_deref())
    );
    assert_ne!(0, person.history.len());
    assert_eq!(before + 1, fed.harness.records(), "one Patient Read record");
    let trail: Actions = document(
        &get(
            &fed.harness,
            &format!("/operator/actions?person={}&kind=view", fed.person),
        )
        .await,
        "the trail",
    );
    assert_eq!(1, trail.actions.len(), "the one view");
    assert_eq!("synthetic-subject-17", trail.actions[0].actor);
    let missing = get(
        &fed.harness,
        "/operator/persons/00000000-0000-7000-8000-000000000000",
    )
    .await;
    assert_eq!(StatusCode::NOT_FOUND, missing.status);
}

#[tokio::test]
async fn the_domains_are_paged_by_system_and_a_domain_shows_its_feeds() {
    if !e2e_enabled() {
        return;
    }
    let fed = Fed::start().await;
    let first: Domains = document(
        &get(&fed.harness, "/operator/domains?limit=2").await,
        "domains",
    );
    assert_eq!(2, first.domains.len());
    let next = first.next.clone().expect("two more domains follow");
    let rest: Domains = document(
        &get(
            &fed.harness,
            &format!("/operator/domains?after={}", urlencode(&next)),
        )
        .await,
        "the next page of domains",
    );
    assert_eq!(2, rest.domains.len());
    assert!(rest.next.is_none());
    let hospital = first
        .domains
        .iter()
        .chain(&rest.domains)
        .find(|domain| domain.system == HOSPITAL_DOMAIN)
        .expect("the hospital domain");
    assert_eq!(1, hospital.records);
    let page: DomainPage = document(
        &get(&fed.harness, &format!("/operator/domains/{}", hospital.id)).await,
        "domain",
    );
    assert_eq!(1, page.feeds.len());
}

#[tokio::test]
async fn the_trail_is_paged_and_its_export_is_csv_recorded_as_an_action() {
    if !e2e_enabled() {
        return;
    }
    let fed = Fed::start().await;
    let first: Actions = document(
        &get(&fed.harness, "/operator/actions?limit=1").await,
        "a page",
    );
    assert_eq!(1, first.actions.len());
    let after = first.next.clone().expect("more actions follow");
    let second: Actions = document(
        &get(
            &fed.harness,
            &format!("/operator/actions?limit=1&after={after}"),
        )
        .await,
        "the next page",
    );
    assert_ne!(first.actions[0].id, second.actions[0].id);
    let before = fed.harness.records();
    let export = post(&fed.harness, "/operator/actions/export", r#"{"limit":1}"#).await;
    assert_eq!(StatusCode::OK, export.status, "{}", export.body);
    assert_eq!(
        Some("no-store"),
        export
            .headers
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
    );
    assert!(export.body.starts_with("id,kind,actor,at,person,"));
    assert_eq!(2, export.body.lines().count(), "the header and one action");
    assert!(export.headers.contains_key(EXPORT_NEXT));
    fed.holds_no_identifier(&export.body, "the export");
    assert_eq!(before + 1, fed.harness.records(), "one Export record");
    let exports: Actions = document(
        &get(&fed.harness, "/operator/actions?kind=export").await,
        "the exports",
    );
    assert_eq!(1, exports.actions.len(), "an export is itself an action");
}

/// Percent-encodes `text` as a query value.
fn urlencode(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}
