// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The packing of the evidence the store held as JSON into the layout of
//! `store::evidence` (#752), behind `FERROPIX_E2E`: over a store set back
//! to where 0044 left it, a server that does not migrate refuses to start,
//! the candidates' JSON column and the links' JSON bytes are packed a batch
//! at a time and read back as the evidence they held, and the rest of the
//! set then drops the JSON.
//!
//! Every name, date and identifier is synthetic.

use ferropix_server::matching::scorer::Evidence;
use ferropix_server::store::{StoreError, evidence, schema};
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN};
use fhir_types::codec::Value as Document;
use sqlx::PgPool;
use uuid::Uuid;

use super::admin;
use super::decisions::{fed, patient};
use super::pixm::{Harness, synthetic_seeds};

/// Each row of `statement` with its packed evidence read back.
async fn evidence_of(pool: &PgPool, statement: &str) -> Vec<(Uuid, Evidence)> {
    let rows: Vec<(Uuid, Vec<u8>)> = sqlx::query_as(sqlx::AssertSqlSafe(statement.to_owned()))
        .fetch_all(pool)
        .await
        .expect("the rows read");
    rows.into_iter()
        .map(|(id, stored)| (id, evidence::unpack(&stored).expect("the evidence reads")))
        .collect()
}

/// Feeds two records the scorer links and two near records it holds as
/// candidates against them.
async fn feed(harness: &Harness) {
    for (system, value) in [(HOSPITAL_DOMAIN, "H-1"), (LABORATORY_DOMAIN, "L-1")] {
        fed(
            harness,
            system,
            value,
            &patient(system, value, "Wolderink", "Sanne", "1966-08-17"),
        )
        .await;
    }
    // Another given name and no address: about 12.6 bits, probable, a candidate.
    for (value, given) in [("P-1", "Bram"), ("P-2", "Cato")] {
        let near: Document = serde_json::from_str(&format!(
            r#"{{"resourceType":"Patient","identifier":[{{"system":"{PRACTICE_DOMAIN}","value":"{value}"}}],"name":[{{"family":"Wolderink","given":["{given}"]}}],"gender":"female","birthDate":"1966-08-17"}}"#
        ))
        .expect("a patient");
        fed(harness, PRACTICE_DOMAIN, value, &near).await;
    }
}

/// Sets the store back to where 0044 left it, with `candidates` and `links`
/// holding their evidence as JSON again.
async fn hold_json(harness: &Harness, candidates: &[(Uuid, Evidence)], links: &[(Uuid, Evidence)]) {
    let pool = harness.store.pool();
    let owner = admin(harness.postgres()).await;
    for statement in [
        "ALTER TABLE pix.link DROP CONSTRAINT link_evidence_packed",
        "ALTER TABLE pix.candidate DROP CONSTRAINT candidate_evidence_packed, ADD COLUMN evidence_json jsonb",
        "ALTER TABLE pix.identifier_domain DROP COLUMN identifier_type",
        "DELETE FROM pix._sqlx_migrations WHERE version >= 45",
    ] {
        sqlx::query(statement)
            .execute(&owner)
            .await
            .expect("the store is set back to where 0044 left it");
    }
    for (id, held) in candidates {
        let json = serde_json::to_string(held).expect("JSON");
        sqlx::query(
            "UPDATE pix.candidate SET evidence_json = $2::jsonb, evidence = NULL WHERE id = $1",
        )
        .bind(id)
        .bind(json)
        .execute(pool)
        .await
        .expect("the candidate holds JSON again");
    }
    for (id, held) in links {
        let json = serde_json::to_string(held).expect("JSON");
        sqlx::query("UPDATE pix.link SET evidence = convert_to($2, 'UTF8') WHERE id = $1")
            .bind(id)
            .bind(json)
            .execute(pool)
            .await
            .expect("the link holds JSON bytes again");
    }
    sqlx::query(
        "ALTER TABLE pix.candidate
             ADD CONSTRAINT candidate_evidence_packed CHECK (evidence IS NOT NULL) NOT VALID",
    )
    .execute(&owner)
    .await
    .expect("the constraint of 0044 holds the rows written from now on");
}

#[tokio::test]
async fn the_evidence_held_as_json_is_packed_a_batch_at_a_time_and_reads_as_it_was() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start_with(synthetic_seeds(), |settings| {
        settings.matching.auto_link = true;
    })
    .await;
    feed(&harness).await;
    let pool = harness.store.pool();
    let candidates = evidence_of(pool, "SELECT id, evidence FROM pix.candidate ORDER BY id").await;
    let links = evidence_of(
        pool,
        "SELECT id, evidence FROM pix.link WHERE evidence IS NOT NULL ORDER BY id",
    )
    .await;
    assert!(candidates.len() >= 2, "candidates to move");
    assert_eq!(1, links.len(), "one probabilistic link");
    hold_json(&harness, &candidates, &links).await;
    assert!(
        matches!(
            schema::check(&harness.store).await,
            Err(StoreError::MigrationsPending { missing: 4 })
        ),
        "a server that does not migrate refuses to start"
    );
    let moved = evidence::refill(pool, 1).await.expect("the refill runs");
    assert_eq!(
        u64::try_from(candidates.len() + links.len()).expect("a count"),
        moved,
        "every row moves, one at a time"
    );
    assert_eq!(0, evidence::refill(pool, 1).await.expect("the refill runs"));
    schema::apply(&harness.store)
        .await
        .expect("the rest of the set applies");
    schema::check(&harness.store)
        .await
        .expect("every migration is applied");
    assert_eq!(
        candidates,
        evidence_of(pool, "SELECT id, evidence FROM pix.candidate ORDER BY id").await,
        "each candidate reads the evidence it held"
    );
    assert_eq!(
        links,
        evidence_of(
            pool,
            "SELECT id, evidence FROM pix.link WHERE evidence IS NOT NULL ORDER BY id"
        )
        .await,
        "the link reads the evidence it held"
    );
    let json: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns
         WHERE table_schema = 'pix' AND column_name = 'evidence_json'",
    )
    .fetch_one(pool)
    .await
    .expect("the columns read");
    assert_eq!(0, json, "the JSON column is gone");
}
