// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! `db reseal`: every identifier column rewritten under a new key, the old
//! key opening and finding nothing afterwards, and a store under another key
//! or mid-reseal refused at start.

use ferropix_server::store::key;
use ferropix_server::store::record::{self, RecordId};
use ferropix_server::store::reseal;
use ferropix_server::store::seal::{IdentifierValue, Sealer};
use ferropix_server::store::{Job, Store, StoreError};
use ferropix_testkit::containers::{e2e_enabled, postgres};
use ferropix_testkit::synthetic::HOSPITAL_DOMAIN;
use uuid::Uuid;

use super::{BSN_SYSTEM, app_role, bsn, create, domain, fresh_key, seeds, settings};

/// One record's identifier columns as stored.
type Columns = (Uuid, Vec<u8>, Option<Vec<u8>>, Vec<u8>);

/// Reads every record's identifier columns, by id.
async fn columns(store: &Store) -> Vec<Columns> {
    sqlx::query_as(
        "SELECT id, identifier_hmac, identifier_sealed, key_id FROM pix.source_record ORDER BY id",
    )
    .fetch_all(store.pool())
    .await
    .unwrap()
}

/// Asserts the record `id` of `value` is found and opened by the new key and
/// by the old one never.
async fn assert_found_by_the_new_key_alone(
    new: &Store,
    old_sealer: &Sealer,
    id: RecordId,
    domain: &record::Domain,
    value: &IdentifierValue,
) {
    assert_eq!(
        Some(id),
        record::find(new.pool(), new.sealer(), domain, value)
            .await
            .unwrap(),
        "the new key finds the record"
    );
    assert_eq!(
        *value,
        record::identifier(new.pool(), new.sealer(), id)
            .await
            .unwrap()
    );
    assert_eq!(
        None,
        record::find(new.pool(), old_sealer, domain, value)
            .await
            .unwrap(),
        "the old key finds nothing"
    );
    if domain.sensitive {
        let opened = record::identifier(new.pool(), old_sealer, id).await;
        assert!(
            matches!(opened, Err(StoreError::Seal(_))),
            "the old key opens nothing: {opened:?}"
        );
    }
}

#[tokio::test]
async fn db_reseal_rewrites_every_identifier_column_and_the_old_key_opens_nothing_afterwards() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let url = app_role(&postgres).await;
    let (old_key, new_key) = (fresh_key(), fresh_key());
    let old = Store::start(&settings(&url, &old_key, true), &seeds(), Job::Migrate)
        .await
        .unwrap();
    let bsn_domain = domain(&old, BSN_SYSTEM).await;
    let hospital = domain(&old, HOSPITAL_DOMAIN).await;
    let mut written: Vec<(RecordId, record::Domain, IdentifierValue)> = Vec::new();
    for index in 0..7 {
        let value = bsn(index);
        written.push((
            create(&old, &bsn_domain, &value, index).await,
            bsn_domain.clone(),
            value,
        ));
    }
    for index in 0..3 {
        let value = IdentifierValue::new(format!("H-{index}")).unwrap();
        written.push((
            create(&old, &hospital, &value, index).await,
            hospital.clone(),
            value,
        ));
    }
    let before = columns(&old).await;
    old.close().await;

    let new = Store::connect(&settings(&url, &new_key, true))
        .await
        .unwrap();
    let old_sealer = Sealer::new(&old_key).unwrap();
    let error = Store::start(&settings(&url, &new_key, true), &seeds(), Job::Migrate)
        .await
        .expect_err("a start under another key is refused");
    assert!(matches!(error, StoreError::KeyMismatch { .. }), "{error:?}");
    let report = reseal::run(&new, &old_sealer, 3).await.unwrap();
    assert_eq!(10, report.records);
    assert_eq!(
        (old_sealer.key_id(), new.sealer().key_id()),
        (report.from, report.to)
    );

    let after = columns(&new).await;
    assert_eq!(before.len(), after.len());
    for ((id, hmac, sealed, key_id), (id_after, hmac_after, sealed_after, key_after)) in
        before.iter().zip(&after)
    {
        assert_eq!(id, id_after);
        assert_ne!(hmac, hmac_after, "the HMAC is rewritten");
        assert_eq!(sealed.is_some(), sealed_after.is_some());
        if let (Some(sealed), Some(sealed_after)) = (sealed, sealed_after) {
            assert_ne!(sealed, sealed_after, "the sealed value is rewritten");
        }
        assert_ne!(key_id, key_after);
        assert_eq!(
            new.sealer().key_id().as_bytes().as_slice(),
            key_after.as_slice()
        );
    }
    for (id, domain, value) in &written {
        assert_found_by_the_new_key_alone(&new, &old_sealer, *id, domain, value).await;
    }
    let stored = key::stored(new.pool()).await.unwrap().unwrap();
    assert_eq!(new.sealer().key_id(), stored.key_id);
    assert_eq!(None, stored.pending);
    new.close().await;

    Store::start(&settings(&url, &new_key, true), &seeds(), Job::Migrate)
        .await
        .expect("the new key starts");
    let refused = Store::start(&settings(&url, &old_key, true), &seeds(), Job::Migrate)
        .await
        .expect_err("the old key no longer starts");
    assert!(
        matches!(refused, StoreError::KeyMismatch { .. }),
        "{refused:?}"
    );
}

#[tokio::test]
async fn a_store_whose_reseal_has_not_finished_refuses_to_start() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let url = app_role(&postgres).await;
    let (old_key, new_key) = (fresh_key(), fresh_key());
    let store = Store::start(&settings(&url, &old_key, true), &seeds(), Job::Migrate)
        .await
        .unwrap();
    let pending = Sealer::new(&new_key).unwrap().key_id();
    sqlx::query("UPDATE pix.seal_key SET pending_key_id = $1")
        .bind(pending.as_bytes().as_slice())
        .execute(store.pool())
        .await
        .unwrap();
    store.close().await;
    for key in [&old_key, &new_key] {
        let error = Store::start(&settings(&url, key, true), &seeds(), Job::Migrate)
            .await
            .expect_err("a store mid-reseal is refused");
        assert!(
            matches!(error, StoreError::ResealPending { .. }),
            "{error:?}"
        );
    }
    let resumed = Store::connect(&settings(&url, &new_key, true))
        .await
        .unwrap();
    let report = reseal::run(&resumed, &Sealer::new(&old_key).unwrap(), reseal::BATCH)
        .await
        .expect("the reseal resumes and finishes");
    assert_eq!(pending, report.to);
    let again = reseal::run(&resumed, &Sealer::new(&old_key).unwrap(), reseal::BATCH)
        .await
        .expect_err("the store is no longer written under the old key");
    assert!(
        matches!(again, StoreError::OldKeyUnknown { .. }),
        "{again:?}"
    );
    let same = reseal::run(&resumed, resumed.sealer(), reseal::BATCH)
        .await
        .expect_err("the configured key is not an old key");
    assert!(matches!(same, StoreError::SameKey), "{same:?}");
    resumed.close().await;
}
