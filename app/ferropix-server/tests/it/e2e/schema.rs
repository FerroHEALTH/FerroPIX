// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The migration set: applied on an empty store and again without change,
//! every relation and constraint of `docs/architecture.md` §4.2 present, the
//! temporal link constraint refusing a second open link, and `serve`
//! migrating only when `[db] migrate` says so.

use ferropix_server::store::schema::migrator;
use ferropix_server::store::{Job, Store, StoreError};
use ferropix_testkit::containers::{e2e_enabled, postgres};
use ferropix_testkit::synthetic::HOSPITAL_DOMAIN;
use uuid::Uuid;

use super::{BSN_SYSTEM, app_role, bsn, create, domain, fresh_key, seeds, settings};

/// Every relation of schema `pix`, the migrator's bookkeeping included.
const RELATIONS: [&str; 18] = [
    "_sqlx_migrations",
    "candidate",
    "carried_identifier",
    "feed_outbox",
    "identifier_domain",
    "link",
    "merge",
    "merge_effect",
    "outbox",
    "person",
    "pseudonym",
    "pseudonym_domain",
    "seal_key",
    "source_record",
    "source_record_version",
    "steward_action",
    "subscription",
    "v2_receipt",
];

/// The constraints the model rests on.
const CONSTRAINTS: [&str; 18] = [
    "carried_identifier_key",
    "merge_kind_check",
    "identifier_domain_system_key",
    "identifier_domain_kind_check",
    "source_record_identifier_key",
    "source_record_replaced_by_fkey",
    "source_record_identifier_check",
    "source_record_head_version_fkey",
    "source_record_version_key",
    "person_merged_into_check",
    "link_one_person_at_a_time",
    "link_ended_by_action_fkey",
    "candidate_a_fkey",
    "merge_persons_check",
    "pseudonym_value_key",
    "seal_key_singleton_check",
    "outbox_channel_check",
    "outbox_settled_check",
];

#[tokio::test]
async fn the_migrations_apply_on_an_empty_store_and_again_without_change() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let url = app_role(&postgres).await;
    let key = fresh_key();
    let store = Store::start(&settings(&url, &key, true), &seeds(), Job::Migrate)
        .await
        .unwrap();
    let applied: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version, checksum FROM pix._sqlx_migrations ORDER BY version")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert_eq!(
        migrator().iter().count(),
        applied.len(),
        "every file is applied once"
    );
    store.close().await;

    let again = Store::start(&settings(&url, &key, true), &seeds(), Job::Migrate)
        .await
        .unwrap();
    let reapplied: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version, checksum FROM pix._sqlx_migrations ORDER BY version")
            .fetch_all(again.pool())
            .await
            .unwrap();
    assert_eq!(applied, reapplied, "a second run applies nothing");

    let relations: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname = 'pix' AND c.relkind IN ('r', 'p')",
    )
    .fetch_all(again.pool())
    .await
    .unwrap();
    for relation in RELATIONS {
        assert!(
            relations.iter().any(|present| present == relation),
            "pix.{relation} is missing"
        );
    }
    let constraints: Vec<String> = sqlx::query_scalar(
        "SELECT c.conname::text FROM pg_constraint c JOIN pg_namespace n ON n.oid = c.connamespace
         WHERE n.nspname = 'pix'",
    )
    .fetch_all(again.pool())
    .await
    .unwrap();
    for constraint in CONSTRAINTS {
        assert!(
            constraints.iter().any(|present| present == constraint),
            "the constraint {constraint} is missing"
        );
    }
    let seeded: Vec<String> =
        sqlx::query_scalar("SELECT system FROM pix.identifier_domain ORDER BY system")
            .fetch_all(again.pool())
            .await
            .unwrap();
    assert_eq!(
        vec![BSN_SYSTEM.to_owned(), HOSPITAL_DOMAIN.to_owned()],
        seeded,
        "the seed is applied once"
    );
    assert_eq!(
        seeds(),
        ferropix_server::store::seed::stored(&again).await.unwrap(),
        "the stored domains read back as seeded"
    );
    sqlx::query("UPDATE pix.identifier_domain SET retired_at = now() WHERE system = $1")
        .bind(HOSPITAL_DOMAIN)
        .execute(again.pool())
        .await
        .unwrap();
    let active: Vec<String> = ferropix_server::store::seed::stored(&again)
        .await
        .unwrap()
        .into_iter()
        .map(|domain| domain.system)
        .collect();
    assert_eq!(
        vec![BSN_SYSTEM.to_owned()],
        active,
        "a retired domain has no Source"
    );
    again.close().await;
}

#[tokio::test]
async fn a_second_open_link_for_one_record_is_refused_by_the_temporal_constraint() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let url = app_role(&postgres).await;
    let store = Store::start(&settings(&url, &fresh_key(), true), &seeds(), Job::Migrate)
        .await
        .unwrap();
    let bsn_domain = domain(&store, BSN_SYSTEM).await;
    let record = create(&store, &bsn_domain, &bsn(0), 0).await;
    let mut persons = Vec::new();
    for _ in 0..2 {
        let person: Uuid = sqlx::query_scalar("INSERT INTO pix.person DEFAULT VALUES RETURNING id")
            .fetch_one(store.pool())
            .await
            .unwrap();
        persons.push(person);
    }
    let link = "INSERT INTO pix.link (record_id, person_id, kind, valid, made_by)
                VALUES ($1, $2, 'feed', tstzrange($3::timestamptz, $4::timestamptz), 'e2e')";
    sqlx::query(link)
        .bind(record.uuid())
        .bind(persons[0])
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-02-01T00:00:00Z")
        .execute(store.pool())
        .await
        .expect("a closed link");
    sqlx::query(link)
        .bind(record.uuid())
        .bind(persons[1])
        .bind("2026-02-01T00:00:00Z")
        .bind(None::<&str>)
        .execute(store.pool())
        .await
        .expect("an open link that starts where the closed one ends");
    let refused = sqlx::query(link)
        .bind(record.uuid())
        .bind(persons[0])
        .bind("2026-03-01T00:00:00Z")
        .bind(None::<&str>)
        .execute(store.pool())
        .await
        .expect_err("a second open link overlaps the first");
    let error = StoreError::from(refused);
    assert_eq!(
        Some("link_one_person_at_a_time"),
        error.database().and_then(|failure| failure.constraint()),
        "{error:?}"
    );
    let deleted = sqlx::query("DELETE FROM pix.link")
        .execute(store.pool())
        .await
        .expect_err("a link is never deleted");
    assert_eq!(
        Some(String::from("23001")),
        StoreError::from(deleted)
            .database()
            .and_then(ferropix_server::store::DbFailure::code),
        "restrict_violation"
    );
    store.close().await;
}

#[tokio::test]
async fn serve_applies_the_pending_migrations_unless_db_migrate_is_false() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let url = app_role(&postgres).await;
    let key = fresh_key();
    let error = Store::start(&settings(&url, &key, false), &seeds(), Job::Serve)
        .await
        .expect_err("an unmigrated store does not serve");
    assert!(matches!(error, StoreError::Query(_)), "{error:?}");
    let probe = Store::connect(&settings(&url, &key, false)).await.unwrap();
    let schema: Option<String> =
        sqlx::query_scalar("SELECT nspname::text FROM pg_namespace WHERE nspname = 'pix'")
            .fetch_optional(probe.pool())
            .await
            .unwrap();
    assert_eq!(None, schema, "migrate = false applies nothing");
    probe.close().await;

    let store = Store::start(&settings(&url, &key, true), &seeds(), Job::Serve)
        .await
        .unwrap();
    let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM pix._sqlx_migrations")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(
        i64::try_from(migrator().iter().count()).unwrap(),
        applied,
        "serve migrated"
    );
    store.close().await;
}
