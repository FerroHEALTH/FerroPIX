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

use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{GIVEN_NAMES, PRACTICE_DOMAIN};
use fhir_types::codec::Value as Document;
use http::StatusCode;
use serde_json::json;

use super::{
    Case, Ceiling, Harness, Meter, POPULATION, analyzed, feed_both, hold_statistics, measure,
    populate, runtime, synthetic_seeds, tables,
};

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
/// and 21,886 allocations.
const CASE: Case = Case {
    name: "ITI-104 revise",
    ceiling: Ceiling {
        statements: 22,
        nested: 1,
        blocks: 2406,
        wal: 4895,
        audit: 0,
        answer: 173,
        allocations: 22_324,
        allocated: 2_621_408,
    },
    expected: StatusCode::OK,
};

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
    let over = measure(
        &runtime,
        &harness,
        &mut meter,
        &CASE,
        &hospital,
        &mut report,
    );
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
