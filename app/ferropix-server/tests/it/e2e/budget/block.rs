// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The budget of an ITI-104 revision whose record shares its probabilistic
//! block with many others (#752): the hospital patient's family name and
//! birth year carried by [`NEAR`] records that grade a candidate against it
//! and [`MATES`] that grade `certainly-not`, so a revision scores and grades
//! about as many pairs as one in the capacity store of a million persons
//! (309 scored, 27 graded `possible` or better). The FHIR case's store holds
//! no such pair, so this case is what holds the work a revision does per
//! scored pair to a ceiling. No specification governs a request's cost: our
//! own design.

use std::fmt::Write as _;

use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{
    GIVEN_NAMES, HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN,
};
use fhir_types::codec::Value as Document;
use http::StatusCode;
use serde_json::json;

use super::{
    Case, Ceiling, Harness, Meter, OWN, POPULATION, admin, analyzed, feed_both, hold_statistics,
    measure, populate, runtime, synthetic_seeds, tables,
};
use crate::e2e::decisions::{fed, patient};

/// The records that grade a candidate against the hospital patient: its
/// family name, birth date and gender with another given name and no
/// address, about 12.6 bits (`docs/architecture.md` §5.2), `probable`.
const NEAR: usize = 27;

/// The records of the same block that grade `certainly-not` against it:
/// the family name and birth year, with another birth date, given name and
/// gender.
const MATES: usize = 282;

/// The ceiling of a revision over the block: the statements as measured,
/// and the shared buffers, the WAL, the bytes and the allocations 2% above
/// the values of three runs (#752): 128 statements, 3412 buffers, 34,696 WAL
/// bytes and 23,230 allocations, 108 of the statements spent on the 27
/// graded pairs (four each: the persons' survival order, the steward's
/// decision, the candidate and its steward action). A revision that states
/// the demographics of its head before leaves each open candidate it scores
/// the same as it is and records no action for it (#752): 101 statements,
/// 2540 buffers, 4799 WAL bytes and 22,897 allocations. The graded pairs'
/// masters read once and their candidates and steward actions written in
/// one statement (#752) leave 22 statements, 2358 buffers, 4802 WAL bytes
/// and 21,886 allocations. Reading the local identifiers in the blocking
/// read (#752) leaves 20 statements, 2041 buffers and 21,785 allocations,
/// and each ceiling fell by the drop. Writing the evidence in the packed
/// layout and not as JSON (#752) took 111 allocations and 211,887 bytes
/// allocated more off, and the two ceilings fell by as much.
const CASE: Case = Case {
    name: "ITI-104 revise",
    ceiling: Ceiling {
        statements: 20,
        nested: 1,
        blocks: 2089,
        wal: 4895,
        audit: 0,
        answer: 173,
        allocations: 22_112,
        allocated: 2_381_106,
    },
    expected: StatusCode::OK,
};

/// The most bytes an open candidate's row and its evidence may hold on
/// average: the evidence in the packed layout of `store::evidence` (#752),
/// measured at 1,922 and 1,800 bytes as JSON on the capacity store, 2%
/// above the 325 and 205 bytes measured.
const CANDIDATE_BYTES: (u64, u64) = (332, 210);

/// The most bytes a probabilistic link's row and its evidence may hold,
/// 2% above the 355 and 219 bytes measured (#752).
const LINK_BYTES: (u64, u64) = (363, 224);

/// The average bytes of the rows of `table` that `filter` keeps and of
/// their evidence, in `harness`'s store.
async fn stored_bytes(harness: &Harness, table: &str, filter: &str) -> (u64, u64) {
    let admin = admin(harness.postgres()).await;
    let (row, evidence): (i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "/* {OWN} */ SELECT avg(pg_column_size(t.*))::bigint, avg(pg_column_size(t.evidence))::bigint
         FROM pix.{table} t WHERE {filter}"
    )))
    .fetch_one(&admin)
    .await
    .expect("the sizes read");
    admin.close().await;
    (
        u64::try_from(row).expect("a size"),
        u64::try_from(evidence).expect("a size"),
    )
}

/// Writes the bytes `measured` of `what` against `ceiling` into `report`,
/// and returns whether one is over it.
fn judge_bytes(what: &str, measured: (u64, u64), ceiling: (u64, u64), report: &mut String) -> bool {
    let mut over = false;
    for (part, value, limit) in [
        ("row", measured.0, ceiling.0),
        ("evidence", measured.1, ceiling.1),
    ] {
        let mark = if value > limit { "  OVER" } else { "" };
        writeln!(
            report,
            "  {what} {part} bytes: {value} (ceiling {limit}){mark}"
        )
        .expect("a String takes it");
        over |= value > limit;
    }
    over
}

/// Feeds the block's records under the practice's domain, each its own
/// person: the [`NEAR`] records, then the [`MATES`].
async fn block(harness: &Harness) {
    for index in 0..NEAR + MATES {
        let value = format!("P-4{index:05}");
        let given = GIVEN_NAMES[1 + index % (GIVEN_NAMES.len() - 1)];
        let patient = if index < NEAR {
            json!({
                "resourceType": "Patient",
                "identifier": [{"system": PRACTICE_DOMAIN, "value": value}],
                "name": [{"family": "Proefsma", "given": [given]}],
                "gender": "female",
                "birthDate": "1961-04-12",
            })
        } else {
            json!({
                "resourceType": "Patient",
                "identifier": [{"system": PRACTICE_DOMAIN, "value": value}],
                "name": [{"family": "Proefsma", "given": [given]}],
                "gender": "male",
                "birthDate": format!("1961-{:02}-2{}", 5 + index % 8, index % 8),
            })
        };
        let patient: Document = serde_json::from_value(patient).expect("a document");
        let fed = harness.feed(PRACTICE_DOMAIN, &value, &patient).await;
        assert_eq!(StatusCode::CREATED, fed.status, "{}", fed.body);
    }
}

#[test]
fn an_iti_104_revision_over_a_block_stays_within_its_budget() {
    if !e2e_enabled() {
        return;
    }
    let runtime = runtime();
    let (harness, hospital, mut meter) = runtime.block_on(async {
        let harness = Harness::start_on_one_connection(synthetic_seeds()).await;
        hold_statistics(&harness).await;
        let (hospital, _bsn) = feed_both(&harness).await;
        populate(&harness, 0..POPULATION).await;
        block(&harness).await;
        let meter = Meter::open(&harness, &harness.spool.path().join("spool")).await;
        (harness, hospital, meter)
    });
    let mut report = format!("over a block of {NEAR} near records and {MATES} mates\n");
    let mut over = measure(
        &runtime,
        &harness,
        &mut meter,
        &CASE,
        &hospital,
        &mut report,
    );
    let candidates = runtime.block_on(stored_bytes(&harness, "candidate", "t.status = 'open'"));
    over |= judge_bytes("open candidate", candidates, CANDIDATE_BYTES, &mut report);
    let analyzed = runtime.block_on(analyzed(&harness));
    assert!(
        analyzed.is_empty(),
        "no table was analyzed during the measurement, which would replan it: {analyzed:?}\n{report}"
    );
    report.push_str(&runtime.block_on(tables(&harness)));
    runtime.block_on(async move { drop((meter, harness)) });
    assert!(
        !over,
        "a revision over a block is over its budget:\n{report}"
    );
    eprintln!("{report}");
}

#[test]
fn a_probabilistic_link_stays_within_its_bytes() {
    if !e2e_enabled() {
        return;
    }
    let runtime = runtime();
    let measured = runtime.block_on(async {
        let harness = Harness::start_with(synthetic_seeds(), |settings| {
            settings.matching.auto_link = true;
        })
        .await;
        for (system, value) in [(HOSPITAL_DOMAIN, "H-1"), (LABORATORY_DOMAIN, "L-1")] {
            fed(
                &harness,
                system,
                value,
                &patient(system, value, "Wolderink", "Sanne", "1966-08-17"),
            )
            .await;
        }
        stored_bytes(
            &harness,
            "link",
            "t.kind = 'probabilistic' AND t.evidence IS NOT NULL",
        )
        .await
    });
    let mut report = String::new();
    let over = judge_bytes("probabilistic link", measured, LINK_BYTES, &mut report);
    assert!(!over, "a link is over its bytes:\n{report}");
    eprintln!("{report}");
}
