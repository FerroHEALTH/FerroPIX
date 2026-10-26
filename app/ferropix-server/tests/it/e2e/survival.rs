// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Which person survives a merge, over PostgreSQL 18 (#103), behind
//! `FERROPIX_E2E`: the person created first survives whatever its id's
//! format, a v4 id sorting below an older v7 one included; a master identity
//! survives any other; and matching never merges two master identities,
//! which it holds apart as a candidate with the `master_identities`
//! conflict for a steward to see.
//!
//! Every name, date and identifier is synthetic.

use ferropix_server::config::section::domain::{DomainKind, DomainSeed};
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN};
use fhir_types::codec::Value as Document;
use uuid::Uuid;

use super::BSN_SYSTEM;
use super::decisions::fed;
use super::pixm::{Harness, SOURCE, synthetic_seeds};

/// The master domain the cases seed.
const MASTER: &str = "https://ferropix.example.org/fhir";

/// A synthetic BSN that passes the eleven-proof.
const BSN: &str = "111222333";

/// The `Patient` of `value` in `system`, carrying the BSN when `bsn`.
fn patient(system: &str, value: &str, family: &str, bsn: bool) -> Document {
    let carried = if bsn {
        format!(r#",{{"system":"{BSN_SYSTEM}","value":"{BSN}"}}"#)
    } else {
        String::new()
    };
    serde_json::from_str(&format!(
        r#"{{"resourceType":"Patient","identifier":[{{"system":"{system}","value":"{value}"}}{carried}],"name":[{{"family":"{family}","given":["Ada"]}}],"gender":"female","birthDate":"1971-03-05"}}"#
    ))
    .expect("a patient")
}

/// The person the open link of the record of `value` names.
async fn person_of(harness: &Harness, value: &str) -> Uuid {
    sqlx::query_scalar(
        "SELECT l.person_id FROM pix.link l
         JOIN pix.source_record r ON r.id = l.record_id
         WHERE upper_inf(l.valid) AND r.identifier_value = $1",
    )
    .bind(value)
    .fetch_one(harness.store.pool())
    .await
    .expect("the record's person")
}

/// The person `person` was merged into, `None` while it is active.
async fn merged_into(harness: &Harness, person: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("SELECT merged_into FROM pix.person WHERE id = $1")
        .bind(person)
        .fetch_one(harness.store.pool())
        .await
        .expect("the person reads")
}

/// The seeds with a plain master domain beside the synthetic ones.
fn with_master() -> Vec<DomainSeed> {
    let mut seeds = synthetic_seeds();
    seeds.push(DomainSeed {
        system: MASTER.to_owned(),
        name: String::from("Master Patient Identification Domain"),
        kind: DomainKind::Master,
        source_client_id: SOURCE.to_owned(),
        sensitive: false,
    });
    seeds
}

#[tokio::test]
async fn the_person_created_first_survives_whatever_the_format_of_its_id() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    fed(
        &harness,
        HOSPITAL_DOMAIN,
        "H-1",
        &patient(HOSPITAL_DOMAIN, "H-1", "Brouwer", true),
    )
    .await;
    let older = person_of(&harness, "H-1").await;
    let younger = Uuid::parse_str("00000000-0000-4000-8000-000000000001").expect("a v4 id");
    assert!(
        younger < older,
        "the younger v4 id sorts below the older v7 one"
    );
    sqlx::query("INSERT INTO pix.person (id) VALUES ($1)")
        .bind(younger)
        .execute(harness.store.pool())
        .await
        .expect("a person with a v4 id");
    fed(
        &harness,
        LABORATORY_DOMAIN,
        "L-1",
        &patient(LABORATORY_DOMAIN, "L-1", "Visser", false),
    )
    .await;
    let own = person_of(&harness, "L-1").await;
    sqlx::query("UPDATE pix.link SET person_id = $1 WHERE person_id = $2 AND upper_inf(valid)")
        .bind(younger)
        .bind(own)
        .execute(harness.store.pool())
        .await
        .expect("the record moved to the v4 person");
    fed(
        &harness,
        LABORATORY_DOMAIN,
        "L-1",
        &patient(LABORATORY_DOMAIN, "L-1", "Visser", true),
    )
    .await;
    assert_eq!(
        None,
        merged_into(&harness, older).await,
        "the older person survives"
    );
    assert_eq!(
        Some(older),
        merged_into(&harness, younger).await,
        "the younger v4 person is merged"
    );
    assert_eq!(older, person_of(&harness, "L-1").await);
}

#[tokio::test]
async fn matching_never_merges_two_master_identities_and_a_master_identity_survives() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(with_master()).await;
    fed(
        &harness,
        MASTER,
        "M-1",
        &patient(MASTER, "M-1", "Brouwer", true),
    )
    .await;
    let first = person_of(&harness, "M-1").await;
    fed(
        &harness,
        MASTER,
        "M-2",
        &patient(MASTER, "M-2", "Brouwer", true),
    )
    .await;
    let second = person_of(&harness, "M-2").await;
    assert_ne!(
        first, second,
        "a second master identity keeps its own person"
    );
    assert_eq!(None, merged_into(&harness, first).await);
    assert_eq!(None, merged_into(&harness, second).await);
    fed(
        &harness,
        HOSPITAL_DOMAIN,
        "H-9",
        &patient(HOSPITAL_DOMAIN, "H-9", "Brouwer", true),
    )
    .await;
    assert_eq!(
        first,
        person_of(&harness, "H-9").await,
        "a local record links to the first master identity"
    );
    assert_eq!(None, merged_into(&harness, first).await);
    assert_eq!(
        None,
        merged_into(&harness, second).await,
        "and the second is not merged into it"
    );
    let held: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pix.candidate
         WHERE status = 'open' AND evidence -> 'conflicts' ? 'master_identities'",
    )
    .fetch_one(harness.store.pool())
    .await
    .expect("the candidates read");
    assert!(
        held >= 1,
        "the master identities are held apart as a candidate"
    );
    let recorded: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pix.steward_action WHERE kind = 'candidate'")
            .fetch_one(harness.store.pool())
            .await
            .expect("the actions read");
    assert!(recorded >= 1, "the candidate is a recorded system action");
}
