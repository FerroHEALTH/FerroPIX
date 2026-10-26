// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Dutch identifiers over PostgreSQL 18 (#117), behind `FERROPIX_E2E`:
//! a pseudonymised BSN held in an authoritative, sealed domain links two
//! records deterministically, is found through ITI-83 and never reaches the
//! log; and a BSN that fails the eleven-proof is refused at the feed.
//!
//! Every name and identifier is synthetic; the pseudonymised BSN is an
//! opaque synthetic value, and no BSNk service is called.

use ferropix_server::config::section::domain::{DomainKind, DomainSeed};
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN};
use fhir_types::codec::Value as Document;
use http::StatusCode;
use nl_generic_functions::identification::PSEUDO_BSN_SYSTEM;

use super::BSN_SYSTEM;
use super::leak::Capture;
use super::pixm::{Harness, SOURCE, synthetic_seeds};

/// A synthetic pseudonymised BSN.
const PSEUDO_BSN: &str = "synthetic-pbsn-7f3a9c";

/// The synthetic seeds with the pseudonymised BSN, authoritative and sealed.
fn with_pseudo_bsn() -> Vec<DomainSeed> {
    let mut seeds = synthetic_seeds();
    seeds.push(DomainSeed {
        system: PSEUDO_BSN_SYSTEM.to_owned(),
        name: String::from("Pseudonymised BSN"),
        kind: DomainKind::Authoritative,
        source_client_id: SOURCE.to_owned(),
        sensitive: true,
    });
    seeds
}

/// The `Patient` of `value` in `system`, carrying `carried` too.
fn patient(system: &str, value: &str, carried: (&str, &str)) -> Document {
    serde_json::from_value(serde_json::json!({
        "resourceType": "Patient",
        "identifier": [
            {"system": system, "value": value},
            {"system": carried.0, "value": carried.1}
        ],
        "name": [{"family": "Ruisdaeltje", "given": ["Ada"]}],
        "gender": "female",
        "birthDate": "1971-03-05"
    }))
    .expect("a patient")
}

#[tokio::test]
async fn a_pseudonymised_bsn_links_two_records_and_never_reaches_the_log() {
    if !e2e_enabled() {
        return;
    }
    let capture = Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());
    let harness = Harness::start(with_pseudo_bsn()).await;
    for (system, value) in [(HOSPITAL_DOMAIN, "H-801"), (LABORATORY_DOMAIN, "L-801")] {
        let fed = harness
            .feed(
                system,
                value,
                &patient(system, value, (PSEUDO_BSN_SYSTEM, PSEUDO_BSN)),
            )
            .await;
        assert_eq!(StatusCode::CREATED, fed.status, "{}", fed.body);
    }
    let answer = harness.pix_get(HOSPITAL_DOMAIN, "H-801", &[]).await;
    assert_eq!(StatusCode::OK, answer.status, "{}", answer.body);
    assert!(
        answer
            .identifiers()
            .contains(&format!("{LABORATORY_DOMAIN}|L-801")),
        "the two records are one patient: {}",
        answer.body
    );
    let by_pseudo_bsn = harness
        .pix_get(PSEUDO_BSN_SYSTEM, PSEUDO_BSN, &[HOSPITAL_DOMAIN])
        .await;
    assert_eq!(
        StatusCode::OK,
        by_pseudo_bsn.status,
        "{}",
        by_pseudo_bsn.body
    );
    assert!(
        by_pseudo_bsn
            .identifiers()
            .contains(&format!("{HOSPITAL_DOMAIN}|H-801"))
    );
    let sealed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pix.carried_identifier c
         JOIN pix.identifier_domain d ON d.id = c.domain_id
         WHERE d.system = $1 AND c.identifier_value IS NULL AND c.identifier_sealed IS NOT NULL",
    )
    .bind(PSEUDO_BSN_SYSTEM)
    .fetch_one(harness.store.pool())
    .await
    .expect("the carried identifiers count");
    assert_eq!(2, sealed, "sealed at rest");
    let logs = capture.text();
    assert!(logs.contains("a feed is applied"), "the capture works");
    assert!(
        !logs.contains(PSEUDO_BSN),
        "the pseudonymised BSN reached the log"
    );
}

#[tokio::test]
async fn a_bsn_that_fails_the_eleven_proof_is_refused_at_the_feed() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let refused = harness
        .feed(
            HOSPITAL_DOMAIN,
            "H-802",
            &patient(HOSPITAL_DOMAIN, "H-802", (BSN_SYSTEM, "111222334")),
        )
        .await;
    assert_eq!(
        StatusCode::UNPROCESSABLE_ENTITY,
        refused.status,
        "{}",
        refused.body
    );
    assert_eq!(
        (
            String::from("business-rule"),
            String::from("an identifier's value is refused by the rule of its domain")
        ),
        refused.issue()
    );
    let records: i64 = sqlx::query_scalar("SELECT count(*) FROM pix.source_record")
        .fetch_one(harness.store.pool())
        .await
        .expect("the records count");
    assert_eq!(0, records, "nothing applied");
}
