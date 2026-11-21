// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The budget of ITI-8 over MLLP: the revision of a known clinic patient,
//! each message with a fresh MSH-10, so none is answered from its receipt,
//! and the `ConceptMap/$translate` calls each sends to the terminology
//! server, held to a ceiling of their own.

use std::fmt::Write as _;

use ferropix_server::hl7v2::cursor::Cursors;
use ferropix_testkit::containers::e2e_enabled;

use super::{Ceiling, Meter, REQUESTS, WARMUP};
use crate::e2e::hl7v2::{V2, adt, code};

/// The ceiling of one ITI-8 revision: the statements as measured, and the
/// shared buffers, the WAL and the bytes 2% above the values measured on
/// 2026-11-21 (#713).
const CEILING: Ceiling = Ceiling {
    statements: 0,
    nested: 0,
    blocks: 98,
    wal: 1964,
    audit: 1444,
    answer: 106,
};

/// The translations one ITI-8 revision asks the terminology server for, one
/// call each (#713).
const TRANSLATIONS: u64 = 2;

/// Sends the revision with control id `control` and returns the bytes of
/// its acknowledgment.
async fn revise(v2: &V2, control: &str) -> u64 {
    let answer = v2
        .send(&adt("A08", control, "H-1^^^HOSP", 0, "19800229"))
        .await;
    assert_eq!(code(&answer), "AA", "{answer}");
    u64::try_from(answer.len()).expect("a length")
}

/// The requests the terminology stub received.
async fn translated(v2: &V2) -> u64 {
    let received = v2
        .terminology
        .0
        .received_requests()
        .await
        .expect("the stub records its requests");
    u64::try_from(received.len()).expect("a count")
}

#[tokio::test]
async fn an_iti_8_revision_stays_within_its_budget() {
    if !e2e_enabled() {
        return;
    }
    let v2 = Box::pin(V2::start(Cursors::default())).await;
    let created = v2
        .send(&adt("A01", "B-0", "H-1^^^HOSP", 0, "19800229"))
        .await;
    assert_eq!(code(&created), "AA", "{created}");
    for warm in 0..WARMUP {
        revise(&v2, &format!("W-{warm}")).await;
    }
    let mut meter = Meter::open(&v2.pix, &v2.spool.path().join("spool")).await;
    meter.start().await;
    let calls_before = translated(&v2).await;
    let mut bytes = 0;
    for request in 0..REQUESTS {
        bytes += revise(&v2, &format!("B-{}", request + 1)).await;
    }
    let calls = translated(&v2).await - calls_before;
    let measured = meter.finish(bytes).await;
    let mut report = String::new();
    let mut over = measured.judge("ITI-8 revise", CEILING, &mut report);
    let each = calls.div_ceil(REQUESTS);
    writeln!(
        report,
        "  translations per message: {each} (ceiling {TRANSLATIONS})"
    )
    .expect("a String takes it");
    over |= each > TRANSLATIONS;
    assert!(!over, "ITI-8 is over its budget:\n{report}");
    eprintln!("{report}");
}
