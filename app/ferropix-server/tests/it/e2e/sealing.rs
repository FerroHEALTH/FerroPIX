// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Sealing at rest: a sealed identifier shows in no column of the store, is
//! found by its HMAC in one probe of the `(domain_id, identifier_hmac)`
//! index, and opens back to the value it was written from.

use ferropix_server::store::record;
use ferropix_server::store::{Job, Store};
use ferropix_testkit::containers::{e2e_enabled, postgres};
use ferropix_testkit::synthetic::HOSPITAL_DOMAIN;

use super::{BSN_SYSTEM, app_role, bsn, create, domain, fresh_key, seeds, settings};

/// The records the probe case writes, enough for the planner to prefer the
/// index over a scan of the table.
const RECORDS: usize = 600;

#[tokio::test]
async fn a_sealed_identifier_is_unreadable_in_a_dump_and_found_in_one_index_probe() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let url = app_role(&postgres).await;
    let store = Store::start(&settings(&url, &fresh_key(), true), &seeds(), Job::Migrate)
        .await
        .unwrap();
    let bsn_domain = domain(&store, BSN_SYSTEM).await;
    let mut transaction = store.pool().begin().await.unwrap();
    for index in 0..RECORDS {
        record::create(
            &mut transaction,
            store.sealer(),
            &bsn_domain,
            &bsn(index),
            &super::demographics(index),
            record::Channel::Fhir,
            None,
        )
        .await
        .unwrap();
    }
    transaction.commit().await.unwrap();
    sqlx::query("ANALYZE pix.source_record")
        .execute(store.pool())
        .await
        .unwrap();

    let dump: Vec<String> = sqlx::query_scalar(
        "SELECT row_to_json(r)::text FROM pix.source_record r
         UNION ALL SELECT row_to_json(v)::text FROM pix.source_record_version v",
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(RECORDS * 2, dump.len());
    for index in 0..RECORDS {
        let digits = bsn(index).expose().to_owned();
        assert!(
            dump.iter().all(|row| !row.contains(&digits)),
            "the BSN at index {index} shows in the dump"
        );
    }

    let wanted = bsn(RECORDS - 100);
    let found = record::find(store.pool(), store.sealer(), &bsn_domain, &wanted)
        .await
        .unwrap()
        .expect("the record is found by its HMAC");
    assert_eq!(
        wanted,
        record::identifier(store.pool(), store.sealer(), found)
            .await
            .unwrap(),
        "the sealed value opens to the value it was written from"
    );
    let plan: String = sqlx::query_scalar(
        "EXPLAIN (FORMAT TEXT, COSTS OFF)
         SELECT id FROM pix.source_record WHERE domain_id = $1 AND identifier_hmac = $2",
    )
    .bind(bsn_domain.id.uuid())
    .bind(
        store
            .sealer()
            .hmac(bsn_domain.id.uuid(), &wanted)
            .as_slice(),
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert!(
        plan.starts_with("Index Scan using source_record_identifier_key")
            || plan.starts_with("Index Only Scan using source_record_identifier_key"),
        "the lookup is one probe of the identifier index: {plan}"
    );
    assert_eq!(
        None,
        record::find(store.pool(), store.sealer(), &bsn_domain, &bsn(RECORDS))
            .await
            .unwrap(),
        "an identifier never fed is not found"
    );
    store.close().await;
}

#[tokio::test]
async fn an_identifier_in_a_domain_that_is_not_sensitive_is_stored_plain_and_found_alike() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let url = app_role(&postgres).await;
    let store = Store::start(&settings(&url, &fresh_key(), true), &seeds(), Job::Migrate)
        .await
        .unwrap();
    let hospital = domain(&store, HOSPITAL_DOMAIN).await;
    assert!(!hospital.sensitive);
    let value = ferropix_server::store::seal::IdentifierValue::new("H-1001").unwrap();
    let id = create(&store, &hospital, &value, 1).await;
    let (sealed, plain): (Option<Vec<u8>>, Option<String>) = sqlx::query_as(
        "SELECT identifier_sealed, identifier_value FROM pix.source_record WHERE id = $1",
    )
    .bind(id.uuid())
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!((None, Some(String::from("H-1001"))), (sealed, plain));
    assert_eq!(
        Some(id),
        record::find(store.pool(), store.sealer(), &hospital, &value)
            .await
            .unwrap()
    );
    store.close().await;
}
