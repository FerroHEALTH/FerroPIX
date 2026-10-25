// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Automatic probabilistic linking over PostgreSQL 18 (#112), behind
//! `FERROPIX_E2E`: with `auto_link` off a certain pair is a candidate and no
//! link; on, the same feed links the records with the score and the evidence
//! on the link, and the dashboard's auto-link count counts only that link.
//!
//! Every name, date and identifier is synthetic.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the link's evidence is read as a JSON value"
)]

use ferropix_operator_api::overview::Overview;
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN};
use serde_json::Value;

use super::decisions::{fed, patient, queue};
use super::operator::{document, get};
use super::pixm::{Harness, synthetic_seeds};

/// Feeds the hospital's and the laboratory's records of one patient, alike
/// enough to grade certain.
async fn twins(harness: &Harness) {
    for (system, value) in [(HOSPITAL_DOMAIN, "H-1"), (LABORATORY_DOMAIN, "L-1")] {
        fed(
            harness,
            system,
            value,
            &patient(system, value, "Wolderink", "Sanne", "1966-08-17"),
        )
        .await;
    }
}

/// The overview the dashboard reads.
async fn overview(harness: &Harness) -> Overview {
    document(&get(harness, "/operator/overview").await, "the overview")
}

#[tokio::test]
async fn with_auto_link_off_a_certain_pair_is_a_candidate_and_no_link() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    twins(&harness).await;
    let queued = queue(&harness, "").await;
    assert_eq!(1, queued.total, "a candidate");
    let probabilistic: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pix.link WHERE kind = 'probabilistic'")
            .fetch_one(harness.store.pool())
            .await
            .expect("the links count");
    assert_eq!(0, probabilistic, "no automatic link");
    let counted = overview(&harness).await;
    assert_eq!(
        (2, 0),
        (counted.records_created_24h, counted.records_linked_24h)
    );
}

#[tokio::test]
async fn with_auto_link_on_the_same_feed_links_with_the_evidence_on_the_link() {
    if !e2e_enabled() {
        return;
    }
    let harness =
        Harness::start_with(synthetic_seeds(), |matching| matching.auto_link = true).await;
    twins(&harness).await;
    assert_eq!(0, queue(&harness, "").await.total, "no candidate");
    let links: Vec<(f64, String)> = sqlx::query_as(
        "SELECT score, evidence::text FROM pix.link
         WHERE kind = 'probabilistic' AND upper_inf(valid)",
    )
    .fetch_all(harness.store.pool())
    .await
    .expect("the links read");
    assert_eq!(1, links.len(), "one record moved by the scorer");
    let (score, evidence) = &links[0];
    assert!(*score >= harness.matching.link_at, "{score}");
    let evidence: Value = serde_json::from_str(evidence).expect("the evidence is JSON");
    assert_eq!("certain", evidence["grade"]);
    assert!(
        evidence["fields"]
            .as_array()
            .is_some_and(|fields| !fields.is_empty())
    );
    let action: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pix.steward_action WHERE kind = 'auto-link'")
            .fetch_one(harness.store.pool())
            .await
            .expect("the actions count");
    assert_eq!(1, action, "recorded as a system action");
    let answered = harness
        .pix_get(HOSPITAL_DOMAIN, "H-1", &[LABORATORY_DOMAIN])
        .await;
    assert!(
        answered
            .identifiers()
            .contains(&format!("{LABORATORY_DOMAIN}|L-1")),
        "{}",
        answered.body
    );
    let counted = overview(&harness).await;
    assert_eq!(
        (2, 1),
        (counted.records_created_24h, counted.records_linked_24h)
    );
}
