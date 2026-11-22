// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The budget of ITI-8 over MLLP: the revision of a known clinic patient,
//! each message with a fresh MSH-10, so none is answered from its receipt,
//! and the `ConceptMap/$translate` calls each sends to the terminology
//! server, held to a ceiling of their own.

use std::fmt::Write as _;

use ferropix_server::hl7v2::cursor::Cursors;
use ferropix_testkit::containers::e2e_enabled;

use super::{Ceiling, Meter, REQUESTS, WARMUP, counted, hold_statistics, runtime};
use crate::e2e::hl7v2::{V2, adt, code};

/// The ceiling of one ITI-8 revision: the statements as measured, and the
/// shared buffers, the WAL and the bytes 2% above the values measured on
/// 2026-11-21 (#713). The search keys (#722) cost a revision one nested
/// statement and 16 shared buffers, the trigger that compares what a search
/// reads. Dropping the hot tables' unread keys (#730) lowered the WAL from
/// 1925 to 1629 bytes, and the blocking keys kept beside the record head
/// (#724) to 1547, for 2 shared buffers more (108 to 110). Keeping the BALP
/// record in the message's own transaction (#742) costs one statement (18
/// to 19), the WAL 1547 to 2261 bytes and the buffers 110 to 114, the
/// record's heap and index pages, and the spool takes none of its 1,444
/// bytes. The allocations and the bytes allocated are 2% above the values
/// of three macOS runs (#721), which CI's Linux runner stays under, and
/// count the test's own MLLP client beside the face. Reading the local
/// identifiers in the blocking read (#752) cost one statement, a buffer,
/// 14 allocations and 1,873 bytes allocated less, and each ceiling fell by
/// as much.
const CEILING: Ceiling = Ceiling {
    statements: 18,
    nested: 1,
    blocks: 116,
    wal: 2307,
    audit: 0,
    answer: 106,
    allocations: 4110,
    allocated: 804_201,
};

/// The terminology requests one ITI-8 revision sends: its translations go
/// in one `batch` (#734).
const TRANSLATIONS: u64 = 1;

/// The revision with control id `control`.
fn revision(control: &str) -> String {
    adt("A08", control, "H-1^^^HOSP", 0, "19800229")
}

/// Sends each of `messages` and returns the bytes of their
/// acknowledgments.
async fn revise(v2: &V2, messages: Vec<String>) -> u64 {
    let mut bytes = 0;
    for message in messages {
        let answer = v2.send(&message).await;
        assert_eq!(code(&answer), "AA", "{answer}");
        bytes += u64::try_from(answer.len()).expect("a length");
    }
    bytes
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

#[test]
fn an_iti_8_revision_stays_within_its_budget() {
    if !e2e_enabled() {
        return;
    }
    let runtime = runtime();
    let (v2, meter, calls_before) = runtime.block_on(async {
        let v2 = Box::pin(V2::start_on_one_connection(Cursors::default())).await;
        hold_statistics(&v2.pix).await;
        let created = v2
            .send(&adt("A01", "B-0", "H-1^^^HOSP", 0, "19800229"))
            .await;
        assert_eq!(code(&created), "AA", "{created}");
        revise(
            &v2,
            (0..WARMUP)
                .map(|warm| revision(&format!("W-{warm}")))
                .collect(),
        )
        .await;
        let mut meter = Meter::open(&v2.pix, &v2.spool.path().join("spool")).await;
        meter.start().await;
        let calls_before = translated(&v2).await;
        (v2, meter, calls_before)
    });
    let messages = (0..REQUESTS)
        .map(|request| revision(&format!("B-{}", request + 1)))
        .collect();
    let (bytes, allocations) = counted(&runtime, revise(&v2, messages));
    let (calls, measured) = runtime.block_on(async {
        let calls = translated(&v2).await - calls_before;
        (calls, meter.finish(bytes).await)
    });
    let measured = measured.with_allocations(allocations);
    runtime.block_on(async move { drop((meter, v2)) });
    let mut report = String::new();
    let mut over = measured.judge("ITI-8 revise", CEILING, &mut report);
    let each = calls.div_ceil(REQUESTS);
    writeln!(
        report,
        "  terminology requests per message: {each} (ceiling {TRANSLATIONS})"
    )
    .expect("a String takes it");
    over |= each > TRANSLATIONS;
    assert!(!over, "ITI-8 is over its budget:\n{report}");
    eprintln!("{report}");
}
