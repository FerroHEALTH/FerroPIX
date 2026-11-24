// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The budget of each hot transaction (#713), behind `FERROPIX_E2E`: what
//! one request costs the store, the audit trail and the wire, held to a
//! ceiling per request so a change that adds a round trip, a statement, a
//! byte of write-ahead log or a byte of audit fails here before a load run
//! finds it.
//!
//! Per request it counts the statements the server sends (the top-level
//! calls `pg_stat_statements` records, `BEGIN` and `COMMIT` included, each a
//! round trip), the statements PostgreSQL runs nested in them (functions,
//! triggers, foreign-key checks; `pg_stat_statements.track = all`), the
//! shared buffers they touch, the write-ahead log of the requests read with
//! `pg_walinspect` (the server writes no full-page images, so a checkpoint
//! adds nothing, and the pruning, vacuum and visibility records are left out,
//! since they run when the xmin horizon lets them, not when a request asks,
//! #731), the bytes of the audit records the spool takes, the bytes of the
//! answer, and the heap allocations and bytes allocated on the thread that
//! runs the router, its tasks and the store's connection (#721). Each container serves one case, so the counts are
//! the case's alone; the test's own reads carry [`OWN`] and are left out,
//! and so are the reads of the WAL position, whose comment the server does
//! not keep. The allocations are counted by `allocation-counter` on a
//! runtime of the test's thread alone, so every task a request wakes is
//! polled where it is counted; the audit spool's writer thread is not, and
//! the count includes the test's own building of each request and reading
//! of each answer. The counter is the test binary's global allocator and
//! counts only inside a measurement, on that thread.
//! Every ceiling is the measured value; a change that lowers one lowers its
//! ceiling in the same pull request. No specification governs a request's
//! cost: our own design.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the fixtures are edited as JSON values"
)]

mod block;
mod iti8;

use std::fmt::Write as _;
use std::future::Future;
use std::path::Path;

use allocation_counter::AllocationInfo;
use axum::body::Body;

use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::fixtures::Fixture;
use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN};
use fhir_types::codec::Value as Document;
use http::{Method, Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

use super::BSN_SYSTEM;
use super::admin;
use super::feeds::condition;
use super::pixm::{
    Answer, Harness, feed_both, fixture, pix_parameters, pix_uri, request, synthetic_seeds,
};
use tokio::runtime::Runtime;

/// The requests of each kind measured.
const REQUESTS: u64 = 20;

/// The requests of each kind sent before the count starts, so every pooled
/// connection has prepared its statements.
const WARMUP: u64 = 4;

/// The patients fed before the count, beside the two the cases ask for, so
/// a search that reads every person reads more than one page.
const POPULATION: usize = 200;

/// The marker every read of the test's own carries.
const OWN: &str = "budget_own";

/// What one request may cost at most.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ceiling {
    /// The statements the server sends.
    pub statements: u64,
    /// The statements nested in them.
    pub nested: u64,
    /// The shared buffers the statements touch.
    pub blocks: u64,
    /// The bytes of write-ahead log.
    pub wal: u64,
    /// The bytes of audit record the spool takes.
    pub audit: u64,
    /// The bytes of the answer.
    pub answer: u64,
    /// The heap allocations.
    pub allocations: u64,
    /// The bytes allocated.
    pub allocated: u64,
}

/// What `REQUESTS` requests of one kind cost.
#[derive(Debug, Default)]
pub(crate) struct Measured {
    statements: u64,
    nested: u64,
    blocks: u64,
    wal: u64,
    audit: u64,
    answer: u64,
    allocations: u64,
    allocated: u64,
    ledger: String,
}

impl Measured {
    /// Adds the allocations `info` counted over the requests.
    pub(crate) fn with_allocations(mut self, info: AllocationInfo) -> Self {
        self.allocations = info.count_total;
        self.allocated = info.bytes_total;
        self
    }

    /// Writes the per-request figures of `name` against `ceiling` into
    /// `report`, and returns whether one is over its ceiling.
    pub(crate) fn judge(&self, name: &str, ceiling: Ceiling, report: &mut String) -> bool {
        let rows = [
            ("statements", self.statements, ceiling.statements),
            ("nested statements", self.nested, ceiling.nested),
            ("shared buffers", self.blocks, ceiling.blocks),
            ("WAL bytes", self.wal, ceiling.wal),
            ("audit bytes", self.audit, ceiling.audit),
            ("answer bytes", self.answer, ceiling.answer),
            ("allocations", self.allocations, ceiling.allocations),
            ("bytes allocated", self.allocated, ceiling.allocated),
        ];
        let mut over = false;
        writeln!(report, "{name}, per request over {REQUESTS}:").expect("a String takes it");
        for (what, total, limit) in rows {
            let each = total.div_ceil(REQUESTS);
            let mark = if each > limit { "  OVER" } else { "" };
            writeln!(report, "  {what:>18}: {each:>7} (ceiling {limit}){mark}")
                .expect("a String takes it");
            over |= each > limit;
        }
        report.push_str(&self.ledger);
        over
    }
}

/// The meter of one case: the superuser's pool on the case's PostgreSQL.
pub(crate) struct Meter {
    admin: PgPool,
    start: String,
    spool: std::path::PathBuf,
    audit_before: u64,
}

impl Meter {
    /// Opens the meter on `harness`'s PostgreSQL, recording the audit
    /// records the spool under `spool` takes.
    pub(crate) async fn open(harness: &Harness, spool: &Path) -> Self {
        let admin = admin(harness.postgres()).await;
        for extension in ["pg_stat_statements", "pg_walinspect"] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "/* {OWN} */ CREATE EXTENSION IF NOT EXISTS {extension}"
            )))
            .execute(&admin)
            .await
            .expect("the extension is created: the testkit loads pg_stat_statements");
        }
        let track: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "/* {OWN} */ SELECT current_setting('pg_stat_statements.track')"
        )))
        .fetch_one(&admin)
        .await
        .expect("the setting reads");
        assert_eq!(track, "all", "the server counts nested statements");
        Self {
            admin,
            start: String::new(),
            spool: spool.to_owned(),
            audit_before: 0,
        }
    }

    /// Starts a count.
    pub(crate) async fn start(&mut self) {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "/* {OWN} */ SELECT pg_stat_statements_reset()"
        )))
        .execute(&self.admin)
        .await
        .expect("the counts reset");
        self.start = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "/* {OWN} */ SELECT pg_current_wal_insert_lsn()::text"
        )))
        .fetch_one(&self.admin)
        .await
        .expect("the WAL position reads");
        self.audit_before = spool_bytes(&self.spool);
    }

    /// Ends the count, with `answer` the bytes of the answers sent.
    pub(crate) async fn finish(&self, answer: u64) -> Measured {
        let rows: Vec<(bool, i64, i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "/* {OWN} */ SELECT toplevel, calls, shared_blks_hit + shared_blks_read,
                    regexp_replace(query, '\\s+', ' ', 'g')
             FROM pg_stat_statements
             WHERE dbid = (SELECT oid FROM pg_database WHERE datname = current_database())
               AND query NOT LIKE '%{OWN}%' AND query NOT LIKE '%pg_stat_statements%'
               AND query NOT LIKE '%pg_current_wal%' AND query NOT LIKE '%pg_get_wal_stats%'
             ORDER BY toplevel DESC, calls DESC, query"
        )))
        .fetch_all(&self.admin)
        .await
        .expect("pg_stat_statements reads");
        let wal: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "/* {OWN} */ SELECT CASE WHEN pg_current_wal_flush_lsn() > $1::pg_lsn THEN (
                 SELECT coalesce(sum(combined_size), 0)::bigint
                 FROM pg_get_wal_stats($1::pg_lsn, pg_current_wal_flush_lsn(), true)
                 WHERE \"resource_manager/record_type\" NOT LIKE 'XLOG/%'
                   AND \"resource_manager/record_type\" NOT LIKE 'Standby/%'
                   AND \"resource_manager/record_type\" NOT LIKE 'Heap2/PRUNE%'
                   AND \"resource_manager/record_type\" NOT LIKE 'Heap2/VACUUM%'
                   AND \"resource_manager/record_type\" NOT LIKE 'Heap2/VISIBLE%'
                   AND \"resource_manager/record_type\" NOT LIKE 'Btree/DELETE%'
                   AND \"resource_manager/record_type\" NOT LIKE 'Btree/VACUUM%'
             ) ELSE 0 END"
        )))
        .bind(&self.start)
        .fetch_one(&self.admin)
        .await
        .expect("pg_walinspect reads");
        let mut measured = Measured {
            wal: count(wal),
            audit: spool_bytes(&self.spool).saturating_sub(self.audit_before),
            answer,
            ..Measured::default()
        };
        for (toplevel, calls, blocks, query) in rows {
            if toplevel {
                measured.statements += count(calls);
            } else {
                measured.nested += count(calls);
            }
            measured.blocks += count(blocks);
            let level = if toplevel { "top" } else { "nested" };
            let text: String = query.chars().take(140).collect();
            writeln!(
                measured.ledger,
                "    {level:>6} {calls:>5} {blocks:>7}  {text}"
            )
            .expect("a String takes it");
        }
        measured
    }
}

/// A runtime on the test's thread alone, so every task a measured request
/// wakes is polled on the thread whose allocations are counted.
pub(crate) fn runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime builds")
}

/// Runs `work` to its end on `runtime`, with the allocations made on this
/// thread meanwhile.
pub(crate) fn counted<T>(runtime: &Runtime, work: impl Future<Output = T>) -> (T, AllocationInfo) {
    let mut out = None;
    let info = allocation_counter::measure(|| out = Some(runtime.block_on(work)));
    (out.expect("the work ran to its end"), info)
}

/// Returns `value` as a count.
fn count(value: i64) -> u64 {
    u64::try_from(value).expect("a count is not negative")
}

/// The bytes of every record the spool under `spool` holds.
fn spool_bytes(spool: &Path) -> u64 {
    std::fs::read_dir(spool)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.metadata().ok())
                .filter(std::fs::Metadata::is_file)
                .map(|metadata| metadata.len())
                .sum()
        })
        .unwrap_or_default()
}

/// Turns autovacuum off on every table of `harness`'s store before a case
/// writes to it, so no automatic analyze replans the measured statements
/// at a moment that depends on the host's timing.
pub(crate) async fn hold_statistics(harness: &Harness) {
    let admin = admin(harness.postgres()).await;
    // NOTE: PostgreSQL CREATE TABLE, storage parameter autovacuum_enabled; no
    // automatic analyze runs, so every run measures the never-analyzed store.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "/* {OWN} */ DO $$
         DECLARE t regclass;
         BEGIN
             FOR t IN SELECT c.oid::regclass FROM pg_catalog.pg_class c
                      WHERE c.relnamespace = 'pix'::regnamespace AND c.relkind = 'r' LOOP
                 EXECUTE format(
                     'ALTER TABLE %s SET (autovacuum_enabled = false, toast.autovacuum_enabled = false)',
                     t
                 );
             END LOOP;
         END
         $$"
    )))
    .execute(&admin)
    .await
    .expect("autovacuum is turned off on the store's tables");
    admin.close().await;
}

/// One kind of FHIR request and its ceiling.
struct Case {
    name: &'static str,
    ceiling: Ceiling,
    expected: StatusCode,
    /// Whether every request carries a token signed for it, so the guard
    /// verifies its signature each time, in place of the one token a client
    /// sends until it expires, whose verdict the guard keeps.
    minted: bool,
}

/// The ceilings of the FHIR face's hot requests: the statements as
/// measured, and the shared buffers, the WAL and the bytes 2% above the
/// values measured (#713). The search keys (#722) lowered ITI-78 from 1062
/// to 101 buffers and from 202 to 0 nested statements, and ITI-119 from
/// 1044 to 587 buffers; their price is the one nested statement an ITI-104
/// revision now spends on the trigger that keeps the keys. Dropping the hot
/// tables' unread keys (#730) lowered an ITI-104 revision's WAL from 2898 to
/// 2439 bytes, and with the domains read once (#715) an ITI-78 search reads
/// 84 buffers. The blocking keys kept beside the record head (#724) lowered
/// ITI-104 from 1106 to 194 buffers and from 2443 to 2277 WAL bytes, and
/// ITI-119 from 588 to 118 buffers. The scoring features kept beside the
/// head (#747) lowered ITI-104 to 190 buffers and ITI-119 to 114. Keeping a
/// revision's BALP record in its own transaction (#742) costs it one
/// statement (19 to 20) and raises its WAL from 2277 to 3197 bytes, the
/// record compressed and sealed in `pix.audit_outbox`, in exchange for the
/// spool's 2,036 bytes and their flush, which it no longer writes. The cases
/// run over a store pool of one connection (#749): the pages a table fills
/// depend on how many backends insert into it, which the pool's rotation
/// decided at a timing of the host's, so the reads now measure 76, 75, 76 and 113 buffers.
/// Leaving an open candidate unchanged when a revision states the
/// demographics of its head before (#752) reads that head's demographics,
/// which raised an ITI-104 revision from 194 to 196 buffers, a price the
/// orchestrator accepted for the block case's drop from 3412 to 2540.
/// Reading the local identifiers in the blocking read (#752) took a
/// statement from ITI-104 and ITI-119, 1 and 25 buffers, 31 and 17
/// allocations and 5,908 and 2,712 bytes allocated, and each ceiling fell
/// by as much.
/// Reading the person's open records once for its own and its carried
/// identifiers, and the subsumed-record check from the chain already read
/// (#762), lowered ITI-83 GET and POST from 76 and 75 buffers to 41 and 40,
/// ITI-78 from 76 to 45 and ITI-119 from 88 to 57, and each ceiling fell by
/// as much.
/// The allocations and the bytes allocated are 2% above the higher of the
/// values on CI's Linux runner and of three macOS runs (#721): the counts
/// were the same in each run, the bytes within 0.1%, and Linux counts 17 to
/// 86 more allocations a request, most of them where a read writes the spool.
/// Each case sends one token, as a client sends its token until it expires,
/// so the guard verifies its signature once and keeps the verdict (#712):
/// that took 60 allocations and 2,724 to 3,199 bytes allocated off every
/// request of every case, and each ceiling fell by as much. The case with a
/// token signed for each request keeps ITI-83 GET's ceilings from before,
/// which held the guard's whole check. ihe-atna 0.25 writes a read's
/// refusal only when its record is withdrawn and every record's bytes with no
/// JSON value between, and ferrohealth-telemetry 0.5.1 formats no span fields
/// (#766): an ITI-83 GET allocates 354 times where it allocated 764, and
/// each case's two ceilings are again 2% above the higher of CI's Linux run
/// and three macOS runs of the same tree.
const CASES: [Case; 6] = [
    Case {
        name: "ITI-104 revise",
        ceiling: Ceiling {
            statements: 18,
            nested: 1,
            blocks: 197,
            wal: 3264,
            audit: 0,
            answer: 173,
            allocations: 1_386,
            allocated: 540_109,
        },
        expected: StatusCode::OK,
        minted: false,
    },
    Case {
        name: "ITI-83 GET",
        ceiling: Ceiling {
            statements: 1,
            nested: 0,
            blocks: 43,
            wal: 0,
            audit: 2188,
            answer: 263,
            allocations: 362,
            allocated: 86_012,
        },
        expected: StatusCode::OK,
        minted: false,
    },
    Case {
        name: "ITI-83 GET, a token signed for it",
        ceiling: Ceiling {
            statements: 1,
            nested: 0,
            blocks: 43,
            wal: 0,
            audit: 2188,
            answer: 263,
            allocations: 433,
            allocated: 89_381,
        },
        expected: StatusCode::OK,
        minted: true,
    },
    Case {
        name: "ITI-83 POST",
        ceiling: Ceiling {
            statements: 1,
            nested: 0,
            blocks: 42,
            wal: 0,
            audit: 2433,
            answer: 361,
            allocations: 1_186,
            allocated: 110_296,
        },
        expected: StatusCode::OK,
        minted: false,
    },
    Case {
        name: "ITI-78 search",
        ceiling: Ceiling {
            statements: 7,
            nested: 0,
            blocks: 47,
            wal: 0,
            audit: 2057,
            // NOTE: #785, draft EEHRxF Annexes Table 26: every identifier carries its type (+534 bytes).
            answer: 1618,
            allocations: 601,
            allocated: 147_255,
        },
        expected: StatusCode::OK,
        minted: false,
    },
    Case {
        name: "ITI-119 match",
        ceiling: Ceiling {
            statements: 7,
            nested: 0,
            blocks: 60,
            wal: 0,
            audit: 2728,
            // NOTE: #785, draft EEHRxF Annexes Table 26: every identifier carries its type (+534 bytes).
            answer: 1601,
            allocations: 1_927,
            allocated: 206_702,
        },
        expected: StatusCode::OK,
        minted: false,
    },
];

/// Feeds the synthetic patients `patients` of the hospital, each its own
/// person.
async fn populate(harness: &Harness, patients: std::ops::Range<usize>) {
    for index in patients {
        let value = format!("H-3{index:05}");
        let patient: Document = serde_json::from_value(json!({
            "resourceType": "Patient",
            "identifier": [{"system": HOSPITAL_DOMAIN, "value": value}],
            "name": [{
                "family": family(index),
                "given": [GIVEN_NAMES[(index * 7) % GIVEN_NAMES.len()]],
            }],
            "gender": if index % 2 == 0 { "female" } else { "male" },
            "birthDate": format!("19{:02}-0{}-1{}", 30 + index % 60, 1 + index % 9, index % 10),
        }))
        .expect("a document");
        let fed = harness.feed(HOSPITAL_DOMAIN, &value, &patient).await;
        assert_eq!(StatusCode::CREATED, fed.status, "{}", fed.body);
    }
}

/// The family name of the synthetic patient `index`: the testkit's names in
/// turn, and past [`POPULATION`] none the ITI-78 case searches, so a larger
/// store adds persons the search must pass over and none it finds.
fn family(index: usize) -> &'static str {
    if index < POPULATION {
        FAMILY_NAMES[index % FAMILY_NAMES.len()]
    } else {
        FAMILY_NAMES[1 + index % (FAMILY_NAMES.len() - 1)]
    }
}

/// One request of the case `name`.
fn build(name: &str, hospital: &str, credential: &str) -> Request<Body> {
    let json = [(header::CONTENT_TYPE, "application/fhir+json")];
    match name {
        "ITI-104 revise" => request(
            Method::PUT,
            &format!("/fhir/Patient?{}", condition(HOSPITAL_DOMAIN, hospital)),
            Some(credential),
            &json,
            Some(serde_json::to_string(&fixture(Fixture::FeedHospital)).expect("JSON")),
        ),
        "ITI-83 GET" | "ITI-83 GET, a token signed for it" => request(
            Method::GET,
            &pix_uri(HOSPITAL_DOMAIN, hospital, &[BSN_SYSTEM]),
            Some(credential),
            &[],
            None,
        ),
        "ITI-83 POST" => request(
            Method::POST,
            "/fhir/Patient/$ihe-pix",
            Some(credential),
            &json,
            Some(pix_parameters(
                HOSPITAL_DOMAIN,
                hospital,
                &[BSN_SYSTEM, ferropix_testkit::synthetic::LABORATORY_DOMAIN],
            )),
        ),
        "ITI-78 search" => request(
            Method::GET,
            "/fhir/Patient?family=Proefsma&birthdate=1961-04-12",
            Some(credential),
            &[],
            None,
        ),
        "ITI-119 match" => {
            let body: Value = serde_json::to_value(fixture(Fixture::MatchIn)).expect("JSON");
            request(
                Method::POST,
                "/fhir/Patient/$match",
                Some(credential),
                &json,
                Some(body.to_string()),
            )
        }
        other => panic!("no case {other}"),
    }
}

/// The credential the case `name` sends: the Source's for a feed, a
/// Consumer's for every other.
fn credential(harness: &Harness, name: &str) -> String {
    if name.starts_with("ITI-104") {
        harness.source()
    } else {
        harness.consumer()
    }
}

/// Sends one request of the case `name`.
async fn send(harness: &Harness, name: &str, hospital: &str) -> Answer {
    let credential = credential(harness, name);
    harness.exchange(build(name, hospital, &credential)).await
}

/// Sends `requests` in turn, asserting each is answered `expected`, and
/// returns the bytes of the answers.
async fn exchange_all(
    harness: &Harness,
    name: &str,
    expected: StatusCode,
    requests: Vec<Request<Body>>,
) -> u64 {
    let mut bytes = 0;
    for request in requests {
        let answer = harness.exchange(request).await;
        assert_eq!(expected, answer.status, "{name}: {}", answer.body);
        bytes += u64::try_from(answer.body.len()).expect("a length");
    }
    bytes
}

/// Sends the warm-up requests of `case`, then counts `REQUESTS` more, and
/// writes their figures against the case's ceiling into `report`; returns
/// whether one is over it.
fn measure(
    runtime: &Runtime,
    harness: &Harness,
    meter: &mut Meter,
    case: &Case,
    hospital: &str,
    report: &mut String,
) -> bool {
    // A client sends one token until it expires, so a case sends one token
    // unless it measures a token signed for each request.
    let reused = credential(harness, case.name);
    let next = || {
        if case.minted {
            credential(harness, case.name)
        } else {
            reused.clone()
        }
    };
    let warm = (0..WARMUP)
        .map(|_| build(case.name, hospital, &next()))
        .collect();
    runtime.block_on(exchange_all(harness, case.name, case.expected, warm));
    // The requests are built before the count, so the test's signing of
    // their tokens is not counted.
    let requests = (0..REQUESTS)
        .map(|_| build(case.name, hospital, &next()))
        .collect();
    runtime.block_on(meter.start());
    let (bytes, allocations) = counted(
        runtime,
        exchange_all(harness, case.name, case.expected, requests),
    );
    let measured = runtime
        .block_on(meter.finish(bytes))
        .with_allocations(allocations);
    measured.judge(case.name, case.ceiling, report)
}

#[test]
fn each_fhir_transaction_stays_within_its_budget() {
    if !e2e_enabled() {
        return;
    }
    let runtime = runtime();
    let (harness, hospital, mut meter) = runtime.block_on(async {
        let harness = Harness::start_on_one_connection(synthetic_seeds()).await;
        hold_statistics(&harness).await;
        let (hospital, _bsn) = feed_both(&harness).await;
        populate(&harness, 0..POPULATION).await;
        let meter = Meter::open(&harness, &harness.spool.path().join("spool")).await;
        (harness, hospital, meter)
    });
    let mut report = String::new();
    let mut over = false;
    for case in &CASES {
        over |= measure(&runtime, &harness, &mut meter, case, &hospital, &mut report);
    }
    let analyzed = runtime.block_on(analyzed(&harness));
    assert!(
        analyzed.is_empty(),
        "no table was analyzed during the measurement, which would replan it: {analyzed:?}\n{report}"
    );
    report.push_str(&runtime.block_on(tables(&harness)));
    runtime.block_on(async move { drop((meter, harness)) });
    assert!(!over, "a transaction is over its budget:\n{report}");
    eprintln!("{report}");
}

/// Each table of `harness`'s store with the rows written to it and its
/// size, which the report shows beside its ledger.
async fn tables(harness: &Harness) -> String {
    let admin = admin(harness.postgres()).await;
    let rows: Vec<(String, i64, i64, i64, i64, i64)> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "/* {OWN} */ SELECT relname::text, n_tup_ins, n_tup_upd, n_tup_del, n_dead_tup,
                pg_total_relation_size(relid)
         FROM pg_catalog.pg_stat_user_tables WHERE schemaname = 'pix' ORDER BY 1"
        )))
        .fetch_all(&admin)
        .await
        .expect("the table counts read");
    admin.close().await;
    let mut text = String::from("  tables (inserted, updated, deleted, dead, bytes):\n");
    for (name, inserted, updated, deleted, dead, bytes) in rows {
        writeln!(
            text,
            "    {name}: {inserted} {updated} {deleted} {dead} {bytes}"
        )
        .expect("a String takes it");
    }
    text
}

/// The tables of `harness`'s store that were vacuumed or analyzed, by hand
/// or automatically, with the statistics each holds.
async fn analyzed(harness: &Harness) -> Vec<(String, f32)> {
    let admin = admin(harness.postgres()).await;
    let rows: Vec<(String, f32)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "/* {OWN} */ SELECT s.relname::text, c.reltuples
         FROM pg_catalog.pg_stat_user_tables s JOIN pg_catalog.pg_class c ON c.oid = s.relid
         WHERE s.schemaname = 'pix' AND (s.last_autoanalyze IS NOT NULL
             OR s.last_analyze IS NOT NULL OR s.last_autovacuum IS NOT NULL
             OR s.last_vacuum IS NOT NULL OR c.reltuples >= 0)
         ORDER BY 1"
    )))
    .fetch_all(&admin)
    .await
    .expect("the table statistics read");
    admin.close().await;
    rows
}

/// The shared buffers `REQUESTS` ITI-78 searches touch, with their ledger.
async fn search_buffers(harness: &Harness, meter: &mut Meter, hospital: &str) -> (u64, String) {
    for _ in 0..WARMUP {
        send(harness, "ITI-78 search", hospital).await;
    }
    meter.start().await;
    for _ in 0..REQUESTS {
        let answer = send(harness, "ITI-78 search", hospital).await;
        assert_eq!(StatusCode::OK, answer.status, "{}", answer.body);
    }
    let measured = meter.finish(0).await;
    (measured.blocks, measured.ledger)
}

/// The pages of the tables a search that scanned every person would read.
async fn scanned_pages(harness: &Harness) -> u64 {
    let admin = admin(harness.postgres()).await;
    let pages: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "/* {OWN} */ SELECT (sum(pg_relation_size(c.oid)) / current_setting('block_size')::bigint)::bigint
         FROM pg_catalog.pg_class c
         WHERE c.relnamespace = 'pix'::regnamespace AND c.relkind = 'r'
           AND c.relname IN ('person', 'person_served', 'person_search_key', 'source_record',
                             'carried_identifier', 'source_record_version')"
    )))
    .fetch_one(&admin)
    .await
    .expect("the table sizes read");
    admin.close().await;
    count(pages)
}

/// An ITI-78 search reads its persons through indexes (#722): over four
/// times the persons, each request reads at most a level more in each index
/// it descends, where a scan of every person would read every page the
/// grown tables added.
#[tokio::test]
async fn an_iti_78_search_reads_no_more_over_four_times_the_persons() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start_on_one_connection(synthetic_seeds()).await;
    hold_statistics(&harness).await;
    let (hospital, _bsn) = feed_both(&harness).await;
    populate(&harness, 0..POPULATION).await;
    let mut meter = Meter::open(&harness, &harness.spool.path().join("spool")).await;
    let pages_before = scanned_pages(&harness).await;
    let (before, small) = search_buffers(&harness, &mut meter, &hospital).await;
    populate(&harness, POPULATION..POPULATION * 4).await;
    let pages_after = scanned_pages(&harness).await;
    let (after, large) = search_buffers(&harness, &mut meter, &hospital).await;
    let report = format!(
        "{REQUESTS} ITI-78 searches over {} persons touch {after} buffers, over {POPULATION} {before}\n  over {POPULATION}:\n{small}  over {}:\n{large}",
        POPULATION * 4,
        POPULATION * 4
    );
    assert!(
        after <= before.saturating_add(REQUESTS.saturating_mul(DESCENTS * PER_DESCENT)),
        "at most a level more in each index the plan descends: {report}"
    );
    let grown = pages_after.saturating_sub(pages_before);
    assert!(
        after.saturating_sub(before).saturating_mul(3) < REQUESTS.saturating_mul(grown),
        "a request reads under a third of the {grown} pages a scan of the grown tables would add: {report}"
    );
    eprintln!("{report}");
}

/// The index descents an ITI-78 search makes for each person it answers.
// NOTE: no specification governs this: our own design; the plan descends person_search_key
// (matched), person_served, source_record and carried_identifier (identified), and person (chain).
const DESCENTS: u64 = 5;

/// The buffers a descent may read more once the store holds four times the
/// persons: one level more of its index, and the leaf a split moved its
/// entries to.
const PER_DESCENT: u64 = 3;
