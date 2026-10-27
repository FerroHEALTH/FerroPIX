// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Pseudonyms over PostgreSQL 18 (#114, #115), behind `FERROPIX_E2E`:
//! - an ITI-83 query from a domain's consumer that names the domain as
//!   `targetSystem` mints the person's pseudonym on the first ask and answers
//!   the same value on every later one, by GET and by POST;
//! - only a domain's consumer reads its pseudonyms (#339): another Consumer
//!   naming the domain gets none and mints none, and a query naming no
//!   `targetSystem` mints nothing and carries the caller's own domains
//!   alone;
//! - a retired domain answers `targetSystem not found`;
//! - parallel first asks yield one pseudonym;
//! - a mint is its own audit record;
//! - a pseudonym resolves to the survivor while merged and back after the
//!   revert;
//! - the reverse resolution is a steward action and a disclosure;
//! - no pseudonym reaches the log.
//!
//! Every name, date and identifier is synthetic.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the answers are read as JSON values"
)]

use std::collections::BTreeSet;

use ferropix_server::config::section::pseudonym::PseudonymSeed;
use ferropix_server::feed::Context;
use ferropix_server::feed::merging::{self, MergeRequest};
use ferropix_server::store::pseudonym;
use ferropix_server::store::record::{Channel, PersonId};
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN};
use http::{Method, StatusCode, header};
use serde_json::Value;
use uuid::Uuid;

use super::decisions::{fed, patient};
use super::leak::Capture;
use super::operator::post;
use super::pixm::{Answer, Harness, pix_parameters, pix_uri, synthetic_seeds};
use crate::auth::bearer;

/// The clinical domain's namespace.
const NAMESPACE: &str = "urn:oid:2.999.7.1";

/// The client the clinical domain's pseudonyms are minted for.
const EHR: &str = "synthetic-ehr";

/// The research domain's namespace.
const RESEARCH: &str = "urn:oid:2.999.7.2";

/// The client the research domain's pseudonyms are minted for.
const REGISTRY: &str = "synthetic-registry";

/// A Consumer that is no pseudonymisation domain's consumer.
const OTHER: &str = "synthetic-consumer";

/// Starts the harness with the clinical pseudonymisation domain.
async fn started() -> Harness {
    let harness = Harness::start(synthetic_seeds()).await;
    pseudonym::seed(
        &harness.store,
        &[PseudonymSeed {
            namespace: NAMESPACE.to_owned(),
            name: String::from("Clinical domain"),
            consumer_client_id: Some(EHR.to_owned()),
        }],
    )
    .await
    .expect("the domain is seeded");
    harness
}

/// A Consumer token of `client`.
fn token(harness: &Harness, client: &str) -> String {
    bearer(&harness.issuer.token(client, "system/Patient.rs"))
}

/// ITI-83 by GET from the hospital's `value`, as `client`, asking `targets`.
async fn ask(harness: &Harness, client: &str, value: &str, targets: &[&str]) -> Answer {
    harness
        .send(
            Method::GET,
            &pix_uri(HOSPITAL_DOMAIN, value, targets),
            Some(&token(harness, client)),
            &[(header::ACCEPT, "application/fhir+json")],
            None,
        )
        .await
}

/// ITI-83 by POST from the hospital's `value`, as `client`, asking `targets`.
async fn ask_posted(harness: &Harness, client: &str, value: &str, targets: &[&str]) -> Answer {
    harness
        .send(
            Method::POST,
            "/fhir/Patient/$ihe-pix",
            Some(&token(harness, client)),
            &[
                (header::CONTENT_TYPE, "application/fhir+json"),
                (header::ACCEPT, "application/fhir+json"),
            ],
            Some(pix_parameters(HOSPITAL_DOMAIN, value, targets)),
        )
        .await
}

/// The pseudonyms of the clinical domain an ITI-83 answer carries.
fn pseudonyms(answer: &Answer) -> Vec<String> {
    pseudonyms_in(answer, NAMESPACE)
}

/// The pseudonyms of the domain `namespace` an ITI-83 answer carries.
fn pseudonyms_in(answer: &Answer, namespace: &str) -> Vec<String> {
    assert_eq!(StatusCode::OK, answer.status, "{}", answer.body);
    answer
        .identifiers()
        .into_iter()
        .filter_map(|identifier| {
            identifier
                .strip_prefix(&format!("{namespace}|"))
                .map(str::to_owned)
        })
        .collect()
}

/// Seeds the research domain beside the clinical one.
async fn research(harness: &Harness) {
    pseudonym::seed(
        &harness.store,
        &[PseudonymSeed {
            namespace: RESEARCH.to_owned(),
            name: String::from("Research domain"),
            consumer_client_id: Some(REGISTRY.to_owned()),
        }],
    )
    .await
    .expect("the research domain is seeded");
}

/// ITI-83 by GET from the laboratory's `L-704`, as the clinical domain's
/// consumer, asking its pseudonym.
async fn lab_ask(harness: &Harness) -> Answer {
    harness
        .send(
            Method::GET,
            &pix_uri(LABORATORY_DOMAIN, "L-704", &[NAMESPACE]),
            Some(&token(harness, EHR)),
            &[(header::ACCEPT, "application/fhir+json")],
            None,
        )
        .await
}

/// The rows of the pseudonym table.
async fn minted(harness: &Harness) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM pix.pseudonym")
        .fetch_one(harness.store.pool())
        .await
        .expect("the pseudonyms count")
}

/// Feeds the hospital's `value` for a patient of `family`.
async fn hospital(harness: &Harness, value: &str, family: &str) {
    fed(
        harness,
        HOSPITAL_DOMAIN,
        value,
        &patient(HOSPITAL_DOMAIN, value, family, "Ada", "1971-03-05"),
    )
    .await;
}

/// The person the record of `value` is in.
async fn person_of(harness: &Harness, value: &str) -> Uuid {
    sqlx::query_scalar(
        "SELECT l.person_id FROM pix.link l JOIN pix.source_record r ON r.id = l.record_id
         WHERE upper_inf(l.valid) AND r.identifier_value = $1",
    )
    .bind(value)
    .fetch_one(harness.store.pool())
    .await
    .expect("the record's person")
}

/// The records of the audit spool that name a minted pseudonym.
fn mint_records(harness: &Harness) -> usize {
    harness
        .audit_texts()
        .iter()
        .filter(|text| text.contains("IHE.BasicAudit.Create") && text.contains("\"pseudonym\""))
        .count()
}

#[tokio::test]
async fn the_consumer_mints_on_the_first_ask_and_reads_the_same_value_after() {
    if !e2e_enabled() {
        return;
    }
    let harness = started().await;
    hospital(&harness, "H-701", "Kroonenberg").await;
    let first = pseudonyms(&ask(&harness, EHR, "H-701", &[NAMESPACE]).await);
    assert_eq!(1, first.len(), "the pseudonym is answered");
    assert!(Uuid::parse_str(&first[0]).is_ok(), "a UUID");
    assert_eq!(1, mint_records(&harness), "the mint is its own record");
    assert_eq!(
        first,
        pseudonyms(&ask_posted(&harness, EHR, "H-701", &[NAMESPACE]).await),
        "the same value by POST"
    );
    assert_eq!(
        first,
        pseudonyms(&ask(&harness, EHR, "H-701", &[NAMESPACE]).await)
    );
    assert_eq!(1, minted(&harness).await);
    assert_eq!(1, mint_records(&harness), "no mint, no record");
    assert_eq!(
        first,
        pseudonyms(&ask(&harness, EHR, "H-701", &[]).await),
        "the consumer's own minted pseudonym is among every identifier"
    );
    assert_eq!(
        first,
        pseudonyms(&ask_posted(&harness, EHR, "H-701", &[]).await),
        "and by POST"
    );
}

#[tokio::test]
async fn another_consumer_never_reads_or_mints_a_domains_pseudonym() {
    if !e2e_enabled() {
        return;
    }
    let harness = started().await;
    hospital(&harness, "H-702", "Kroonenberg").await;
    let before = ask(&harness, OTHER, "H-702", &[NAMESPACE]).await;
    assert_eq!(
        Vec::<String>::new(),
        pseudonyms(&before),
        "no pseudonym, none minted"
    );
    assert_eq!(
        Vec::<String>::new(),
        pseudonyms(&ask(&harness, EHR, "H-702", &[]).await),
        "a query naming no targetSystem mints nothing"
    );
    assert_eq!(0, minted(&harness).await);
    let value = pseudonyms(&ask(&harness, EHR, "H-702", &[NAMESPACE]).await);
    assert_eq!(1, value.len(), "the consumer mints");
    for answer in [
        ask(&harness, OTHER, "H-702", &[NAMESPACE]).await,
        ask_posted(&harness, OTHER, "H-702", &[NAMESPACE]).await,
        ask(&harness, OTHER, "H-702", &[]).await,
        ask_posted(&harness, OTHER, "H-702", &[]).await,
    ] {
        assert_eq!(
            Vec::<String>::new(),
            pseudonyms(&answer),
            "another Consumer never reads the minted value"
        );
        assert!(
            !answer.body.contains(&value[0]),
            "the pseudonym is nowhere in the answer"
        );
    }
    assert_eq!(1, minted(&harness).await, "another Consumer mints nothing");
}

#[tokio::test]
async fn a_query_naming_no_target_carries_the_callers_own_domain_alone() {
    if !e2e_enabled() {
        return;
    }
    let harness = started().await;
    research(&harness).await;
    hospital(&harness, "H-708", "Kroonenberg").await;
    let clinical = pseudonyms(&ask(&harness, EHR, "H-708", &[NAMESPACE]).await);
    let registry = pseudonyms_in(
        &ask(&harness, REGISTRY, "H-708", &[RESEARCH]).await,
        RESEARCH,
    );
    assert_eq!(
        (1, 1),
        (clinical.len(), registry.len()),
        "one in each domain"
    );
    assert_eq!(2, minted(&harness).await);
    for answer in [
        ask(&harness, EHR, "H-708", &[]).await,
        ask_posted(&harness, EHR, "H-708", &[]).await,
    ] {
        assert_eq!(clinical, pseudonyms(&answer), "the caller's own domain");
        assert_eq!(
            Vec::<String>::new(),
            pseudonyms_in(&answer, RESEARCH),
            "never another domain's"
        );
    }
    for answer in [
        ask(&harness, REGISTRY, "H-708", &[]).await,
        ask_posted(&harness, REGISTRY, "H-708", &[]).await,
    ] {
        assert_eq!(registry, pseudonyms_in(&answer, RESEARCH));
        assert_eq!(Vec::<String>::new(), pseudonyms(&answer));
    }
    let crossed = ask(&harness, EHR, "H-708", &[RESEARCH]).await;
    assert_eq!(
        Vec::<String>::new(),
        pseudonyms_in(&crossed, RESEARCH),
        "naming another consumer's domain reads nothing"
    );
    assert_eq!(2, minted(&harness).await, "nothing more is minted");
}

#[tokio::test]
async fn a_retired_domain_is_no_target_and_parallel_first_asks_mint_one() {
    if !e2e_enabled() {
        return;
    }
    let harness = started().await;
    hospital(&harness, "H-703", "Kroonenberg").await;
    let (first, second, third, fourth, fifth, sixth) = tokio::join!(
        ask(&harness, EHR, "H-703", &[NAMESPACE]),
        ask(&harness, EHR, "H-703", &[NAMESPACE]),
        ask_posted(&harness, EHR, "H-703", &[NAMESPACE]),
        ask(&harness, EHR, "H-703", &[NAMESPACE]),
        ask_posted(&harness, EHR, "H-703", &[NAMESPACE]),
        ask(&harness, EHR, "H-703", &[NAMESPACE]),
    );
    let values: BTreeSet<Vec<String>> = [first, second, third, fourth, fifth, sixth]
        .iter()
        .map(pseudonyms)
        .collect();
    assert_eq!(1, values.len(), "one value for every ask: {values:?}");
    assert_eq!(1, minted(&harness).await, "one pseudonym");
    assert_eq!(1, mint_records(&harness), "one mint, one record");

    sqlx::query("UPDATE pix.pseudonym_domain SET retired_at = now()")
        .execute(harness.store.pool())
        .await
        .expect("retired");
    let refused = ask(&harness, EHR, "H-703", &[NAMESPACE]).await;
    assert_eq!(StatusCode::FORBIDDEN, refused.status, "{}", refused.body);
    assert_eq!("targetSystem not found", refused.issue().1);
}

#[tokio::test]
async fn a_pseudonym_resolves_to_the_survivor_while_merged_and_back_after_the_revert() {
    if !e2e_enabled() {
        return;
    }
    let capture = Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());
    let harness = started().await;
    hospital(&harness, "H-704", "Kroonenberg").await;
    fed(
        &harness,
        LABORATORY_DOMAIN,
        "L-704",
        &patient(LABORATORY_DOMAIN, "L-704", "Verhoeven", "Bo", "1980-07-08"),
    )
    .await;
    let survivor = person_of(&harness, "H-704").await;
    let merged = person_of(&harness, "L-704").await;
    let kept = pseudonyms(&ask(&harness, EHR, "H-704", &[NAMESPACE]).await);
    let own = pseudonyms(&lab_ask(&harness).await);
    assert_ne!(kept, own, "two persons, two pseudonyms");

    let steward = Context {
        actor: String::from("synthetic-steward"),
        channel: Channel::Fhir,
        request_id: None,
    };
    let version = |id: Uuid| {
        let store = harness.store.clone();
        async move {
            let mut connection = store.pool().acquire().await.expect("a connection");
            merging::version(&mut connection, PersonId::from_uuid(id))
                .await
                .expect("reads")
                .expect("a person")
        }
    };
    let request = MergeRequest {
        survivor: PersonId::from_uuid(survivor),
        survivor_version: version(survivor).await,
        merged: PersonId::from_uuid(merged),
        merged_version: version(merged).await,
        reason: String::from("one patient"),
    };
    let (transaction, change) = merging::merge(&harness.store, &request, &steward)
        .await
        .expect("merged");
    transaction.commit().await.expect("committed");
    assert_eq!(
        kept,
        pseudonyms(&lab_ask(&harness).await),
        "the merged identifier answers the survivor's pseudonym"
    );
    let resolve = |value: &str| {
        serde_json::json!({
            "namespace": NAMESPACE,
            "pseudonym": value,
            "reason": "Synthfamily asked by phone"
        })
        .to_string()
    };
    let resolved = post(&harness, "/operator/pseudonyms/resolve", &resolve(&own[0])).await;
    assert_eq!(StatusCode::OK, resolved.status, "{}", resolved.body);
    let resolution: Value = serde_json::from_str(&resolved.body).expect("JSON");
    assert_eq!(
        survivor.to_string(),
        resolution["person"],
        "the merged person's pseudonym resolves to the survivor"
    );

    let (transaction, _reverted) = merging::revert(
        &harness.store,
        change.merge,
        &version(survivor).await,
        "two patients",
        &steward,
    )
    .await
    .expect("reverted");
    transaction.commit().await.expect("committed");
    assert_eq!(own, pseudonyms(&lab_ask(&harness).await), "its own again");
    let resolved = post(&harness, "/operator/pseudonyms/resolve", &resolve(&own[0])).await;
    let resolution: Value = serde_json::from_str(&resolved.body).expect("JSON");
    assert_eq!(merged.to_string(), resolution["person"]);

    let actions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pix.steward_action WHERE kind = 'pseudonym-resolve' AND reason IS NOT NULL",
    )
    .fetch_one(harness.store.pool())
    .await
    .expect("the actions count");
    assert_eq!(2, actions, "each resolution is a steward action");
    let disclosures = harness
        .audit_texts()
        .iter()
        .filter(|text| text.contains("PrivacyDisclosure") && text.contains("pseudonym-resolution"))
        .count();
    assert_eq!(2, disclosures, "and a disclosure record");
    let logs = capture.text();
    for value in kept.iter().chain(&own) {
        assert!(
            !logs.contains(value.as_str()),
            "a pseudonym reached the log"
        );
    }
}

#[tokio::test]
async fn an_administrator_adds_a_domain_and_a_steward_mints_in_it() {
    if !e2e_enabled() {
        return;
    }
    let harness = started().await;
    hospital(&harness, "H-705", "Kroonenberg").await;
    let created = harness
        .send(
            Method::POST,
            "/operator/pseudonym-domains",
            Some(&harness.admin()),
            &[(header::CONTENT_TYPE, "application/json")],
            Some(
                serde_json::json!({
                    "namespace": "urn:oid:2.999.7.2",
                    "name": "Research",
                    "reason": "a study"
                })
                .to_string(),
            ),
        )
        .await;
    assert_eq!(StatusCode::CREATED, created.status, "{}", created.body);
    let domain: Value = serde_json::from_str(&created.body).expect("JSON");
    let taken = harness
        .send(
            Method::POST,
            "/operator/pseudonym-domains",
            Some(&harness.admin()),
            &[(header::CONTENT_TYPE, "application/json")],
            Some(serde_json::json!({"namespace": HOSPITAL_DOMAIN, "name": "Clash"}).to_string()),
        )
        .await;
    assert_eq!(
        StatusCode::CONFLICT,
        taken.status,
        "an identifier domain holds it"
    );
    let person = person_of(&harness, "H-705").await;
    let body =
        serde_json::json!({"person": person.to_string(), "domain": domain["id"]}).to_string();
    let first = post(&harness, "/operator/pseudonyms", &body).await;
    assert_eq!(StatusCode::OK, first.status, "{}", first.body);
    let first: Value = serde_json::from_str(&first.body).expect("JSON");
    assert_eq!(true, first["created"]);
    let again: Value =
        serde_json::from_str(&post(&harness, "/operator/pseudonyms", &body).await.body)
            .expect("JSON");
    assert_eq!(
        false, again["created"],
        "a second mint returns the held one"
    );
    let listed = harness
        .send(
            Method::GET,
            "/operator/pseudonym-domains",
            Some(&harness.steward()),
            &[],
            None,
        )
        .await;
    let listed: Value = serde_json::from_str(&listed.body).expect("JSON");
    let research = listed["domains"]
        .as_array()
        .expect("domains")
        .iter()
        .find(|domain| domain["namespace"] == "urn:oid:2.999.7.2")
        .expect("listed");
    assert_eq!(1, research["pseudonyms"]);
}
