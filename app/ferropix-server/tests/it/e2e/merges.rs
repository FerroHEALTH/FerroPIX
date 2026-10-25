// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A steward's merge and its revert over PostgreSQL 18 (#108), behind
//! `FERROPIX_E2E`: merges and reverts unwind in order with the ITI-83
//! answers checked after each step, the preview names what the merge then
//! does, and a feed between the preview and the merge makes it `409`.
//!
//! Every name, date and identifier is synthetic.

use ferropix_operator_api::merge::{Outcome, Preview};
use ferropix_operator_api::search::Results;
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN};
use http::StatusCode;

use super::decisions::{fed, patient, queue};
use super::operator::{document, get, post};
use super::pixm::{Answer, Harness, synthetic_seeds};

/// Feeds `value` in `system` as a patient of its own and returns its person.
async fn person(harness: &Harness, system: &str, value: &str, family: &str, birth: &str) -> String {
    fed(
        harness,
        system,
        value,
        &patient(system, value, family, "Mira", birth),
    )
    .await;
    let found: Results = document(
        &post(
            harness,
            "/operator/search/identifier",
            &format!(r#"{{"system":"{system}","value":"{value}"}}"#),
        )
        .await,
        "the search",
    );
    found.persons[0].id.clone()
}

/// Reads the preview of merging `merged` into `survivor`.
async fn preview(harness: &Harness, survivor: &str, merged: &str) -> Preview {
    document(
        &get(
            harness,
            &format!("/operator/merges/preview?survivor={survivor}&merged={merged}"),
        )
        .await,
        "the merge preview",
    )
}

/// Reads the preview of reverting `merge`.
async fn revert_preview(harness: &Harness, merge: &str) -> Preview {
    document(
        &get(harness, &format!("/operator/merges/preview?revert={merge}")).await,
        "the revert preview",
    )
}

/// Merges as `seen` previewed it.
async fn merge(harness: &Harness, seen: &Preview) -> Answer {
    post(
        harness,
        "/operator/merges",
        &format!(
            r#"{{"survivor":"{}","survivor_version":"{}","merged":"{}","merged_version":"{}","reason":"synthetic merge"}}"#,
            seen.survivor.id, seen.survivor.version, seen.merged.id, seen.merged.version
        ),
    )
    .await
}

/// Reverts `merge` as `seen` previewed it.
async fn revert(harness: &Harness, merge: &str, seen: &Preview) -> Answer {
    post(
        harness,
        &format!("/operator/merges/{merge}/revert"),
        &format!(
            r#"{{"survivor_version":"{}","reason":"synthetic revert"}}"#,
            seen.survivor.version
        ),
    )
    .await
}

/// The `system|value` identifiers ITI-83 answers for the hospital's `H-1`.
async fn cross_referenced(harness: &Harness) -> Vec<String> {
    let answer = harness
        .pix_get(
            HOSPITAL_DOMAIN,
            "H-1",
            &[LABORATORY_DOMAIN, PRACTICE_DOMAIN],
        )
        .await;
    answer.identifiers().into_iter().collect()
}

#[tokio::test]
async fn merges_and_reverts_unwind_in_order_and_iti_83_follows_each_step() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let a = person(&harness, HOSPITAL_DOMAIN, "H-1", "Ostendorp", "1950-01-02").await;
    let b = person(
        &harness,
        LABORATORY_DOMAIN,
        "L-1",
        "Quaadvliet",
        "1962-05-06",
    )
    .await;
    let c = person(&harness, PRACTICE_DOMAIN, "P-1", "Ruwhorst", "1977-09-10").await;
    assert!(cross_referenced(&harness).await.is_empty(), "three persons");
    let seen = preview(&harness, &a, &b).await;
    assert_eq!(
        1,
        seen.identifiers.len(),
        "the laboratory's identifier moves"
    );
    let before = harness.records();
    let first: Outcome = document(&merge(&harness, &seen).await, "the first merge");
    assert_eq!(before + 1, harness.records(), "one BALP record per merge");
    assert_eq!(1, first.moved);
    assert_eq!(
        vec![format!("{LABORATORY_DOMAIN}|L-1")],
        cross_referenced(&harness).await
    );

    let seen = preview(&harness, &a, &c).await;
    let second: Outcome = document(&merge(&harness, &seen).await, "the second merge");
    assert_eq!(
        vec![
            format!("{LABORATORY_DOMAIN}|L-1"),
            format!("{PRACTICE_DOMAIN}|P-1")
        ],
        cross_referenced(&harness).await
    );

    let out_of_order = revert_preview(&harness, &first.merge).await;
    assert_eq!(Some(second.merge.clone()), out_of_order.first_revert);
    let refused = revert(&harness, &first.merge, &out_of_order).await;
    assert_eq!(StatusCode::CONFLICT, refused.status, "{}", refused.body);
    assert!(refused.body.contains("reverted first"), "{}", refused.body);

    let seen = revert_preview(&harness, &second.merge).await;
    assert_eq!(None, seen.first_revert);
    assert_eq!(
        1,
        seen.identifiers.len(),
        "the practice's identifier moves back"
    );
    let undone: Outcome = document(&revert(&harness, &second.merge, &seen).await, "a revert");
    assert_eq!(c, undone.merged, "the practice's person stands again");
    assert_eq!(
        vec![format!("{LABORATORY_DOMAIN}|L-1")],
        cross_referenced(&harness).await
    );
    let again = revert(&harness, &second.merge, &seen).await;
    assert_eq!(StatusCode::CONFLICT, again.status, "a merge reverts once");

    let seen = revert_preview(&harness, &first.merge).await;
    document::<Outcome>(
        &revert(&harness, &first.merge, &seen).await,
        "the first revert",
    );
    assert!(
        cross_referenced(&harness).await.is_empty(),
        "three persons again"
    );
    let counted: (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE kind = 'merge'), count(*) FILTER (WHERE kind = 'unmerge')
         FROM pix.steward_action WHERE reason IS NOT NULL",
    )
    .fetch_one(harness.store.pool())
    .await
    .expect("the actions count");
    assert_eq!((2, 2), counted);
}

#[tokio::test]
async fn the_preview_names_what_the_merge_then_does() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let a = person(
        &harness,
        HOSPITAL_DOMAIN,
        "H-1",
        "Steenwijkel",
        "1981-02-03",
    )
    .await;
    let b = person(
        &harness,
        LABORATORY_DOMAIN,
        "L-1",
        "Steenwijkel",
        "1981-02-03",
    )
    .await;
    let queued = queue(&harness, "").await;
    assert_eq!(1, queued.total, "the two records are a candidate");
    let seen = preview(&harness, &a, &b).await;
    assert_eq!(vec![queued.rows[0].id.clone()], seen.candidates);
    let moved: Vec<String> = seen
        .identifiers
        .iter()
        .map(|row| row.domain.system.clone())
        .collect();
    let made: Outcome = document(&merge(&harness, &seen).await, "the merge");
    assert_eq!(seen.identifiers.len(), usize::try_from(made.moved).unwrap());
    assert_eq!(vec![LABORATORY_DOMAIN.to_owned()], moved);
    assert_eq!(0, queue(&harness, "").await.total, "the candidate closed");
    let status: String = sqlx::query_scalar("SELECT status FROM pix.candidate WHERE id = $1::uuid")
        .bind(&seen.candidates[0])
        .fetch_one(harness.store.pool())
        .await
        .expect("the candidate reads");
    assert_eq!("linked", status);
}

#[tokio::test]
async fn a_feed_between_the_preview_and_the_merge_makes_it_409() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let a = person(&harness, HOSPITAL_DOMAIN, "H-1", "Grevelink", "1944-11-12").await;
    let b = person(
        &harness,
        LABORATORY_DOMAIN,
        "L-1",
        "Haarbrink",
        "1958-07-08",
    )
    .await;
    let seen = preview(&harness, &a, &b).await;
    fed(
        &harness,
        LABORATORY_DOMAIN,
        "L-1",
        &patient(
            LABORATORY_DOMAIN,
            "L-1",
            "Haarbrink",
            "Mirabel",
            "1958-07-08",
        ),
    )
    .await;
    let refused = merge(&harness, &seen).await;
    assert_eq!(StatusCode::CONFLICT, refused.status, "{}", refused.body);
    for body in [
        format!(
            r#"{{"survivor":"{a}","survivor_version":"x","merged":"{a}","merged_version":"x","reason":"r"}}"#
        ),
        format!(
            r#"{{"survivor":"{a}","survivor_version":"x","merged":"{b}","merged_version":"x","reason":" "}}"#
        ),
    ] {
        let answer = post(&harness, "/operator/merges", &body).await;
        assert_eq!(StatusCode::BAD_REQUEST, answer.status, "{body}");
    }
    let unknown = get(
        &harness,
        &format!(
            "/operator/merges/preview?survivor={a}&merged=0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"
        ),
    )
    .await;
    assert_eq!(StatusCode::NOT_FOUND, unknown.status);
    let malformed = get(&harness, &format!("/operator/merges/preview?survivor={a}")).await;
    assert_eq!(StatusCode::BAD_REQUEST, malformed.status);
}
