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
//! #731), the bytes of the audit records the spool takes, and the
//! bytes of the answer. Each container serves one case, so the counts are
//! the case's alone; the test's own reads carry [`OWN`] and are left out,
//! and so are the reads of the WAL position, whose comment the server does
//! not keep.
//! Every ceiling is the measured value; a change that lowers one lowers its
//! ceiling in the same pull request. No specification governs a request's
//! cost: our own design.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the fixtures are edited as JSON values"
)]

mod iti8;

use std::fmt::Write as _;
use std::path::Path;

use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::fixtures::Fixture;
use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN};
use fhir_types::codec::Value as Document;
use http::{Method, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

use super::BSN_SYSTEM;
use super::admin;
use super::pixm::{Answer, Harness, feed_both, fixture, pix_parameters, synthetic_seeds};

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
    ledger: String,
}

impl Measured {
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
/// ITI-119 from 588 to 118 buffers. Keeping a revision's BALP record in its
/// own transaction (#742) costs it one statement (19 to 20) and raises its
/// WAL from 2277 to 3200 bytes, the record compressed and sealed in
/// `pix.audit_outbox`, in exchange for the spool's 2,036 bytes and their
/// flush, which it no longer writes.
const CASES: [Case; 5] = [
    Case {
        name: "ITI-104 revise",
        ceiling: Ceiling {
            statements: 20,
            nested: 1,
            blocks: 198,
            wal: 3264,
            audit: 0,
            answer: 173,
        },
        expected: StatusCode::OK,
    },
    Case {
        name: "ITI-83 GET",
        ceiling: Ceiling {
            statements: 1,
            nested: 0,
            blocks: 79,
            wal: 0,
            audit: 2188,
            answer: 263,
        },
        expected: StatusCode::OK,
    },
    Case {
        name: "ITI-83 POST",
        ceiling: Ceiling {
            statements: 1,
            nested: 0,
            blocks: 79,
            wal: 0,
            audit: 2433,
            answer: 361,
        },
        expected: StatusCode::OK,
    },
    Case {
        name: "ITI-78 search",
        ceiling: Ceiling {
            statements: 7,
            nested: 0,
            blocks: 86,
            wal: 0,
            audit: 2057,
            answer: 1084,
        },
        expected: StatusCode::OK,
    },
    Case {
        name: "ITI-119 match",
        ceiling: Ceiling {
            statements: 8,
            nested: 0,
            blocks: 121,
            wal: 0,
            audit: 2728,
            answer: 1067,
        },
        expected: StatusCode::OK,
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

/// Sends one request of the case `name`.
async fn send(harness: &Harness, name: &str, hospital: &str) -> Answer {
    match name {
        "ITI-104 revise" => {
            harness
                .feed(HOSPITAL_DOMAIN, hospital, &fixture(Fixture::FeedHospital))
                .await
        }
        "ITI-83 GET" => {
            harness
                .pix_get(HOSPITAL_DOMAIN, hospital, &[BSN_SYSTEM])
                .await
        }
        "ITI-83 POST" => {
            harness
                .pix_post(&pix_parameters(
                    HOSPITAL_DOMAIN,
                    hospital,
                    &[BSN_SYSTEM, ferropix_testkit::synthetic::LABORATORY_DOMAIN],
                ))
                .await
        }
        "ITI-78 search" => {
            harness
                .send(
                    Method::GET,
                    "/fhir/Patient?family=Proefsma&birthdate=1961-04-12",
                    Some(&harness.consumer()),
                    &[],
                    None,
                )
                .await
        }
        "ITI-119 match" => {
            let body: Value = serde_json::to_value(fixture(Fixture::MatchIn)).expect("JSON");
            harness
                .send(
                    Method::POST,
                    "/fhir/Patient/$match",
                    Some(&harness.consumer()),
                    &[(header::CONTENT_TYPE, "application/fhir+json")],
                    Some(body.to_string()),
                )
                .await
        }
        other => panic!("no case {other}"),
    }
}

#[tokio::test]
async fn each_fhir_transaction_stays_within_its_budget() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    hold_statistics(&harness).await;
    let (hospital, _bsn) = feed_both(&harness).await;
    populate(&harness, 0..POPULATION).await;
    let mut meter = Meter::open(&harness, &harness.spool.path().join("spool")).await;
    let mut report = String::new();
    let mut over = false;
    for case in &CASES {
        for _ in 0..WARMUP {
            let answer = send(&harness, case.name, &hospital).await;
            assert_eq!(
                case.expected, answer.status,
                "{}: {}",
                case.name, answer.body
            );
        }
        meter.start().await;
        let mut bytes = 0;
        for _ in 0..REQUESTS {
            let answer = send(&harness, case.name, &hospital).await;
            assert_eq!(
                case.expected, answer.status,
                "{}: {}",
                case.name, answer.body
            );
            bytes += u64::try_from(answer.body.len()).expect("a length");
        }
        let measured = meter.finish(bytes).await;
        over |= measured.judge(case.name, case.ceiling, &mut report);
    }
    assert!(!over, "a transaction is over its budget:\n{report}");
    eprintln!("{report}");
}

/// The shared buffers `REQUESTS` ITI-78 searches touch.
async fn search_buffers(harness: &Harness, meter: &mut Meter, hospital: &str) -> u64 {
    for _ in 0..WARMUP {
        send(harness, "ITI-78 search", hospital).await;
    }
    meter.start().await;
    for _ in 0..REQUESTS {
        let answer = send(harness, "ITI-78 search", hospital).await;
        assert_eq!(StatusCode::OK, answer.status, "{}", answer.body);
    }
    meter.finish(0).await.blocks
}

/// An ITI-78 search reads its persons through indexes (#722): over four
/// times the persons, each request reads at most a few more buffers, the
/// deeper levels of the indexes it descends, where a scan of every person
/// would read about four times as many.
#[tokio::test]
async fn an_iti_78_search_reads_no_more_over_four_times_the_persons() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    hold_statistics(&harness).await;
    let (hospital, _bsn) = feed_both(&harness).await;
    populate(&harness, 0..POPULATION).await;
    let mut meter = Meter::open(&harness, &harness.spool.path().join("spool")).await;
    let before = search_buffers(&harness, &mut meter, &hospital).await;
    populate(&harness, POPULATION..POPULATION * 4).await;
    let after = search_buffers(&harness, &mut meter, &hospital).await;
    assert!(
        after <= before.saturating_add(REQUESTS.saturating_mul(DEEPER)),
        "{REQUESTS} ITI-78 searches over {} persons touch {after} buffers, over {POPULATION} {before}",
        POPULATION * 4
    );
}

/// The buffers one request may read more once the store holds four times
/// the persons: a level more in each of the indexes it descends.
const DEEPER: u64 = 8;
