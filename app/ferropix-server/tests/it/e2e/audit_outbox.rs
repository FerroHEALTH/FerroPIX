// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The audit outbox over PostgreSQL 19 (#742), behind `FERROPIX_E2E`: a
//! write's BALP record commits with its change or not at all, a refused
//! write keeps the record of its refusal and changes nothing, a write killed
//! before its commit leaves neither, the changes of one ITI-93 message
//! commit together and are tried again together, and the forwarder delivers
//! each committed record once and sets a refused one aside.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the records are read as JSON values"
)]

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use ferrohealth_health::registry::HealthIndicator as _;
use ferropix_server::audit::records::Call;
use ferropix_server::audit::{Audit, Quarantine};
use ferropix_server::config::section::domain::{DomainKind, DomainSeed};
use ferropix_server::config::section::matching::Matching;
use ferropix_server::feed::master::{MasterChange, MasterStep, Outcomes};
use ferropix_server::feed::{Applied, FeedError, Feeder, Recording, feed_record};
use ferropix_server::store::record::PersonId;
use ferropix_server::store::{Job, Store};
use ferropix_testkit::containers::{Postgres, e2e_enabled, postgres};
use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN};
use ihe_atna::balp::{Exchange, Outcome};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;
use wiremock::matchers;
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::feeds::{self, Fed, Patient, SOURCE, context, feeder, start};
use super::{admin, app_role, fresh_key, outbox_records, settings};
use crate::auth::Captured;

/// The master domain the ITI-93 cases write.
const MASTER: &str = "http://localhost:8080";

/// The public base the records name.
const BASE: &str = "https://pix.example.org/fhir";

/// The call a Source makes, as the records name it.
fn call() -> Call {
    let access = ferrohealth_smart::resource::Access::Caller(Arc::new(
        ferrohealth_smart::resource::Caller::new(
            SOURCE.to_owned(),
            Some(SOURCE.to_owned()),
            "system/Patient.cud",
            None,
            false,
            ferrohealth_smart::kind::CallerKind::System,
        ),
    ));
    Call::of(
        &access,
        None,
        &url::Url::parse(BASE).expect("a URL"),
        Some("e2e"),
    )
}

/// The records the audit outbox holds, as JSON.
async fn kept(audit: &Audit) -> Vec<Value> {
    outbox_records(audit)
        .await
        .iter()
        .map(|record| serde_json::from_slice(record).expect("a record is JSON"))
        .collect()
}

/// The rows of `table` in schema `pix`.
async fn count(store: &Store, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM pix.{table}"
    )))
    .fetch_one(store.pool())
    .await
    .expect("the count reads")
}

#[tokio::test]
async fn a_feed_keeps_its_record_in_its_own_transaction() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let fed = feeder(&started.store);
    let call = call();
    let patient = Patient::new(HOSPITAL_DOMAIN, "H-742", FAMILY_NAMES[2], GIVEN_NAMES[3]);
    let revision = patient.revision();
    let records = |outcome: Result<&Applied, &FeedError>| {
        vec![feed_record(&call, revision.identifier(), false, outcome)]
    };
    let applied = fed
        .feeder
        .revise(&revision, &context(), Recording::new(&fed.audit, &records))
        .await
        .expect("the feed commits");
    let records = kept(&fed.audit).await;
    assert_eq!(1, records.len(), "one record in the audit outbox");
    assert_eq!(
        "https://profiles.ihe.net/ITI/PIXm/StructureDefinition/IHE.PIXm.Feed.Create.Audit.Manager",
        records[0]["meta"]["profile"][0]
    );
    assert_eq!("0", records[0]["outcome"]);
    let person = format!("Patient/{}", applied.person.expect("a person").uuid());
    assert!(
        records[0].to_string().contains(&person),
        "the person is the data"
    );
    assert_eq!(1, count(&started.store, "source_record").await);
    let sealed: Vec<u8> = sqlx::query_scalar("SELECT record_sealed FROM pix.audit_outbox")
        .fetch_one(started.store.pool())
        .await
        .expect("the row reads");
    assert!(
        !String::from_utf8_lossy(&sealed).contains("H-742"),
        "the record is sealed at rest"
    );
    started.store.close().await;
}

#[tokio::test]
async fn a_refused_feed_keeps_the_record_of_its_refusal_and_changes_nothing() {
    if !e2e_enabled() {
        return;
    }
    let started = start().await;
    let fed = feeder(&started.store);
    let call = call();
    let unknown = Patient::new(
        "urn:oid:2.999.742.9",
        "U-1",
        FAMILY_NAMES[1],
        GIVEN_NAMES[1],
    );
    let revision = unknown.revision();
    let records = |outcome: Result<&Applied, &FeedError>| {
        vec![feed_record(&call, revision.identifier(), false, outcome)]
    };
    let refused = fed
        .feeder
        .revise(&revision, &context(), Recording::new(&fed.audit, &records))
        .await;
    assert!(
        matches!(refused, Err(FeedError::UnknownDomain)),
        "{refused:?}"
    );
    let records = kept(&fed.audit).await;
    assert_eq!(1, records.len(), "the refusal is recorded once");
    assert_eq!("4", records[0]["outcome"], "as a failure");
    assert_eq!(0, count(&started.store, "source_record").await);
    started.store.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_feed_killed_before_its_commit_leaves_neither_its_change_nor_its_record() {
    if !e2e_enabled() {
        return;
    }
    let mastered = start_mastered().await;
    let admin = mastered.admin.clone();
    let fed = &mastered.fed;
    let call = call();
    let patient = Patient::new(HOSPITAL_DOMAIN, "H-743", FAMILY_NAMES[3], GIVEN_NAMES[4]);
    let revision = patient.revision();
    let (written, in_transaction) = mpsc::channel::<()>();
    let (killed, dead) = mpsc::channel::<()>();
    let dead = Mutex::new(dead);
    let killing = tokio::spawn(kill_when(admin.clone(), in_transaction, killed));
    let records = |outcome: Result<&Applied, &FeedError>| {
        // NOTE: no specification governs this: our own design; the change is written
        // and uncommitted here, where the kill of the session stands for a crash.
        written.send(()).expect("the killer waits");
        dead.lock()
            .expect("one feed")
            .recv()
            .expect("the session is killed");
        vec![feed_record(&call, revision.identifier(), false, outcome)]
    };
    let crashed = fed
        .feeder
        .revise(&revision, &context(), Recording::new(&fed.audit, &records))
        .await;
    killing.await.expect("the kill ends");
    assert!(
        matches!(crashed, Err(FeedError::Unrecorded(_))),
        "{crashed:?}"
    );
    assert_eq!(
        0,
        count(&mastered.store, "source_record").await,
        "no change"
    );
    assert_eq!(0, count(&mastered.store, "audit_outbox").await, "no record");
    mastered.admin.close().await;
    mastered.store.close().await;
}

/// Kills the store's session that holds an open transaction once `written`
/// says the feed wrote its change, then says so on `killed`.
async fn kill_when(admin: PgPool, written: mpsc::Receiver<()>, killed: mpsc::Sender<()>) {
    tokio::task::spawn_blocking(move || written.recv())
        .await
        .expect("the wait ends")
        .expect("the feed writes");
    let ended: Vec<bool> = sqlx::query_scalar(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity
         WHERE state = 'idle in transaction' AND datname = current_database()",
    )
    .fetch_all(&admin)
    .await
    .expect("the session is killed");
    assert_eq!(vec![true], ended, "the feed's session, alone");
    killed.send(()).expect("the feed waits");
}

/// A started store whose seeds hold the master domain.
struct Mastered {
    _postgres: Postgres,
    store: Store,
    fed: Fed,
    admin: PgPool,
}

/// Starts PostgreSQL and the store with the feed seeds and the master
/// domain.
async fn start_mastered() -> Mastered {
    let postgres = postgres().await.expect("PostgreSQL starts");
    let url = app_role(&postgres).await;
    let mut seeds = feeds::seeds();
    seeds.push(DomainSeed {
        system: MASTER.to_owned(),
        name: String::from("Master Patient Identification Domain"),
        kind: DomainKind::Master,
        source_client_id: SOURCE.to_owned(),
        sensitive: false,
    });
    let store = Store::start(&settings(&url, &fresh_key(), true), &seeds, Job::Migrate)
        .await
        .expect("the store starts");
    let fed = Fed::with(
        Feeder::new(
            store.clone(),
            &Matching::default().resolve().expect("the defaults resolve"),
        ),
        &store,
    );
    let admin = admin(&postgres).await;
    Mastered {
        _postgres: postgres,
        store,
        fed,
        admin,
    }
}

/// The Patient of the master identity `id`.
fn master_patient(id: Uuid, family: &str) -> fhir_types::r4::patient::Patient {
    serde_json::from_value(serde_json::json!({
        "resourceType": "Patient",
        "id": id.to_string(),
        "name": [{"family": family, "given": [GIVEN_NAMES[0]]}],
        "birthDate": "1970-01-01"
    }))
    .expect("a Patient")
}

/// The one record a message keeps, whatever its outcomes.
fn one_record(_outcomes: &Outcomes) -> Vec<Exchange> {
    vec![ferropix_server::audit::records::pmir_feed(
        &call(),
        Vec::new(),
        Some("742"),
        Outcome::Success,
    )]
}

/// Whether the person `id` is held.
async fn held(store: &Store, id: Uuid) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pix.person WHERE id = $1)")
        .bind(id)
        .fetch_one(store.pool())
        .await
        .expect("the person reads")
}

#[tokio::test]
async fn a_failure_at_the_second_change_rolls_back_the_first_and_keeps_no_record() {
    if !e2e_enabled() {
        return;
    }
    let mastered = start_mastered().await;
    let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
    for statement in [
        format!(
            "CREATE FUNCTION pix.refuse_second() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 IF NEW.id = '{second}' THEN RAISE EXCEPTION 'the second change fails'; END IF;
                 RETURN NEW;
             END $$"
        ),
        String::from(
            "CREATE TRIGGER refuse_second BEFORE INSERT ON pix.person
             FOR EACH ROW EXECUTE FUNCTION pix.refuse_second()",
        ),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(&mastered.admin)
            .await
            .expect("the trigger is made");
    }
    let (a, b) = (
        master_patient(first, "Eerst"),
        master_patient(second, "Tweede"),
    );
    let changes = [
        Some(MasterStep::Write {
            change: MasterChange::Create,
            id: PersonId::from_uuid(first),
            patient: &a,
        }),
        Some(MasterStep::Write {
            change: MasterChange::Create,
            id: PersonId::from_uuid(second),
            patient: &b,
        }),
    ];
    let failed = mastered
        .fed
        .feeder
        .message(
            MASTER,
            &changes,
            &context(),
            &mastered.fed.audit,
            &one_record,
        )
        .await;
    assert!(matches!(failed, Err(FeedError::Store(_))), "{failed:?}");
    assert!(
        !held(&mastered.store, first).await,
        "the first change rolled back"
    );
    assert!(!held(&mastered.store, second).await);
    assert_eq!(0, count(&mastered.store, "source_record").await);
    assert_eq!(0, count(&mastered.store, "audit_outbox").await, "no record");
    mastered.admin.close().await;
    mastered.store.close().await;
}

#[tokio::test]
async fn a_message_broken_by_a_deadlock_is_tried_again_whole_and_recorded_once() {
    if !e2e_enabled() {
        return;
    }
    let mastered = start_mastered().await;
    let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
    for statement in [
        String::from("CREATE SEQUENCE pix.attempts"),
        format!(
            "CREATE FUNCTION pix.deadlock_once() RETURNS trigger LANGUAGE plpgsql AS $$
             BEGIN
                 IF NEW.id = '{second}' AND nextval('pix.attempts') = 1 THEN
                     RAISE EXCEPTION 'a deadlock' USING ERRCODE = '40P01';
                 END IF;
                 RETURN NEW;
             END $$"
        ),
        String::from(
            "CREATE TRIGGER deadlock_once BEFORE INSERT ON pix.person
             FOR EACH ROW EXECUTE FUNCTION pix.deadlock_once()",
        ),
        String::from("GRANT USAGE ON SEQUENCE pix.attempts TO PUBLIC"),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(&mastered.admin)
            .await
            .expect("the trigger is made");
    }
    let (a, b) = (
        master_patient(first, "Eerst"),
        master_patient(second, "Tweede"),
    );
    let unknown = PersonId::from_uuid(Uuid::new_v4());
    let changes = [
        Some(MasterStep::Write {
            change: MasterChange::Update,
            id: unknown,
            patient: &a,
        }),
        Some(MasterStep::Write {
            change: MasterChange::Create,
            id: PersonId::from_uuid(first),
            patient: &a,
        }),
        Some(MasterStep::Write {
            change: MasterChange::Create,
            id: PersonId::from_uuid(second),
            patient: &b,
        }),
    ];
    let outcomes = mastered
        .fed
        .feeder
        .message(
            MASTER,
            &changes,
            &context(),
            &mastered.fed.audit,
            &one_record,
        )
        .await
        .expect("the second attempt commits");
    assert!(
        matches!(outcomes.first(), Some(Some(Err(FeedError::NotFound)))),
        "a change refused on its own leaves the others: {outcomes:?}"
    );
    assert!(held(&mastered.store, first).await);
    assert!(held(&mastered.store, second).await);
    let attempts: i64 = sqlx::query_scalar("SELECT last_value FROM pix.attempts")
        .fetch_one(&mastered.admin)
        .await
        .expect("the sequence reads");
    assert_eq!(2, attempts, "the whole message was tried again once");
    assert_eq!(
        1,
        kept(&mastered.fed.audit).await.len(),
        "the message is recorded once"
    );
    mastered.admin.close().await;
    mastered.store.close().await;
}

#[tokio::test]
async fn the_forwarder_delivers_each_record_once_and_sets_a_refused_one_aside() {
    if !e2e_enabled() {
        return;
    }
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let started = start().await;
    let repository = MockServer::start().await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/fhir/AuditEvent"))
        .respond_with(ResponseTemplate::new(201))
        .up_to_n_times(1)
        .mount(&repository)
        .await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/fhir/AuditEvent"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&repository)
        .await;
    let spool = tempfile::tempdir().expect("a directory");
    let audit = Audit::open(
        &crate::audit::spool::audit_settings(
            &spool.path().join("spool"),
            Some(&format!("{}/fhir", repository.uri())),
            10,
        ),
        &url::Url::parse(BASE).expect("a URL"),
    )
    .and_then(|audit| audit.with_store(&started.store))
    .expect("the audit trail opens");
    let draining = audit.drain().expect("a repository is configured");
    let fed = Fed::with(
        Feeder::new(
            started.store.clone(),
            &Matching::default().resolve().expect("the defaults resolve"),
        ),
        &started.store,
    );
    let call = call();
    let send = |value: &'static str| {
        let patient = Patient::new(HOSPITAL_DOMAIN, value, FAMILY_NAMES[4], GIVEN_NAMES[5]);
        let audit = &audit;
        let call = &call;
        let feeder = &fed.feeder;
        async move {
            let revision = patient.revision();
            let records = |outcome: Result<&Applied, &FeedError>| {
                vec![feed_record(call, revision.identifier(), false, outcome)]
            };
            feeder
                .revise(&revision, &context(), Recording::new(audit, &records))
                .await
                .expect("the feed commits");
        }
    };
    send("H-744").await;
    assert!(
        eventually(|| async { count(&started.store, "audit_outbox").await == 0 }).await,
        "the delivered record is deleted"
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        1,
        repository
            .received_requests()
            .await
            .unwrap_or_default()
            .len(),
        "the record is sent once"
    );
    let health = Quarantine(audit.clone());
    send("H-745").await;
    assert!(
        eventually(|| async { count(&started.store, "audit_quarantine").await == 1 }).await,
        "the refused record is set aside"
    );
    assert_eq!(0, count(&started.store, "audit_outbox").await);
    assert!(
        eventually(|| async {
            health.check().await.state() == ferrohealth_health::document::State::Down
        })
        .await,
        "a quarantined record takes the indicator down"
    );
    let id: i64 = sqlx::query_scalar("SELECT id FROM pix.audit_quarantine")
        .fetch_one(started.store.pool())
        .await
        .expect("the row reads");
    let log = String::from_utf8_lossy(&captured.0.lock().expect("the buffer")).into_owned();
    assert!(
        log.contains("moved to the quarantine") && log.contains(&format!("id={id}")),
        "the quarantine is logged by id: {log}"
    );
    assert!(!log.contains("H-745"), "no log names the identifier");
    draining.abort();
    started.store.close().await;
}

/// Waits at most ten seconds for `condition`.
async fn eventually<F, Fut>(condition: F) -> bool
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    for _ in 0..200 {
        if condition().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    condition().await
}
