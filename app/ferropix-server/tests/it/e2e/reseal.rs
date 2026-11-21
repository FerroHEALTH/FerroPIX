// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! `db reseal`: every identifier column rewritten under a new key, the old
//! key opening and finding nothing afterwards, and a store under another key
//! or mid-reseal refused at start, and the outbox's payloads rewritten too.

use ferropix_server::store::key;
use ferropix_server::store::outbox::{self, Channel, Message};
use ferropix_server::store::record::{self, RecordId};
use ferropix_server::store::reseal;
use ferropix_server::store::seal::{IdentifierValue, Sealer};
use ferropix_server::store::{Job, Store, StoreError};
use ferropix_testkit::containers::{e2e_enabled, postgres};
use ferropix_testkit::synthetic::HOSPITAL_DOMAIN;
use secrecy::{ExposeSecret as _, SecretString};
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

#[tokio::test]
async fn db_reseal_rewrites_the_payloads_waiting_in_the_outbox() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let url = app_role(&postgres).await;
    let (old_key, new_key) = (fresh_key(), fresh_key());
    let old = Store::start(&settings(&url, &old_key, true), &seeds(), Job::Migrate)
        .await
        .unwrap();
    let message = Message {
        channel: Channel::Xcpd,
        destination: String::from("https://ig.example.org/replies"),
        media_type: "application/soap+xml; charset=UTF-8",
        payload: SecretString::from(String::from("<answer>a synthetic patient</answer>")),
        max_attempts: 3,
        subscription: None,
        consumer: None,
    };
    outbox::enqueue(old.pool(), old.sealer(), &message)
        .await
        .unwrap();
    old.close().await;

    let new = Store::connect(&settings(&url, &new_key, true))
        .await
        .unwrap();
    reseal::run(&new, &Sealer::new(&old_key).unwrap(), reseal::BATCH)
        .await
        .unwrap();
    let key_id: Vec<u8> = sqlx::query_scalar("SELECT key_id FROM pix.outbox")
        .fetch_one(new.pool())
        .await
        .unwrap();
    assert_eq!(
        new.sealer().key_id().as_bytes().as_slice(),
        key_id.as_slice()
    );
    let claimed = outbox::claim(&new, 10, outbox::LEASE).await.unwrap();
    assert_eq!(1, claimed.len(), "the payload opens under the new key");
    assert_eq!(
        "<answer>a synthetic patient</answer>",
        claimed[0].payload.expose_secret()
    );
}

/// A rotation over more carried identifiers than one batch walks them by
/// their key (#730); interrupted by one it cannot open, it keeps the batches
/// it committed, and resumed it finishes, every identifier found by the new
/// key alone.
#[tokio::test]
async fn an_interrupted_rotation_over_many_carried_identifiers_resumes_and_finishes() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let url = app_role(&postgres).await;
    let (old_key, new_key) = (fresh_key(), fresh_key());
    let old = Store::start(
        &settings(&url, &old_key, true),
        &super::feeds::seeds(),
        Job::Migrate,
    )
    .await
    .unwrap();
    let feeder = super::feeds::feeder(&old);
    let carried: Vec<IdentifierValue> = (0..8).map(bsn).collect();
    for (index, value) in carried.iter().enumerate() {
        let patient = super::feeds::Patient::new(
            HOSPITAL_DOMAIN,
            &format!("H-7{index:03}"),
            ferropix_testkit::synthetic::FAMILY_NAMES[index % 8],
            ferropix_testkit::synthetic::GIVEN_NAMES[index % 8],
        )
        .carrying(BSN_SYSTEM, value.expose());
        feeder
            .revise(&patient.revision(), &super::feeds::context())
            .await
            .unwrap();
    }
    let bsn_domain = domain(&old, BSN_SYSTEM).await;
    old.close().await;

    let new = Store::connect(&settings(&url, &new_key, true))
        .await
        .unwrap();
    let old_sealer = Sealer::new(&old_key).unwrap();
    let last: (Uuid, i32, Uuid, Vec<u8>, Vec<u8>) = sqlx::query_as(
        "SELECT record_id, version, domain_id, identifier_hmac, identifier_sealed
         FROM pix.carried_identifier WHERE identifier_sealed IS NOT NULL
         ORDER BY record_id DESC, version DESC, domain_id DESC, identifier_hmac DESC LIMIT 1",
    )
    .fetch_one(new.pool())
    .await
    .unwrap();
    let corrupt = |sealed: Vec<u8>| {
        sqlx::query(
            "UPDATE pix.carried_identifier SET identifier_sealed = $5
             WHERE record_id = $1 AND version = $2 AND domain_id = $3 AND identifier_hmac = $4",
        )
        .bind(last.0)
        .bind(last.1)
        .bind(last.2)
        .bind(last.3.clone())
        .bind(sealed)
    };
    corrupt(vec![0_u8; last.4.len()])
        .execute(new.pool())
        .await
        .unwrap();
    let interrupted = reseal::run(&new, &old_sealer, 2).await;
    assert!(
        matches!(interrupted, Err(StoreError::Seal(_))),
        "the rotation stops at the identifier it cannot open: {interrupted:?}"
    );
    let resealed: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pix.carried_identifier WHERE key_id = $1")
            .bind(new.sealer().key_id().as_bytes().as_slice())
            .fetch_one(new.pool())
            .await
            .unwrap();
    assert!(resealed > 2, "the batches before it are kept: {resealed}");

    corrupt(last.4.clone()).execute(new.pool()).await.unwrap();
    reseal::run(&new, &old_sealer, 2)
        .await
        .expect("the resumed rotation finishes");
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pix.carried_identifier WHERE key_id <> $1")
            .bind(new.sealer().key_id().as_bytes().as_slice())
            .fetch_one(new.pool())
            .await
            .unwrap();
    assert_eq!(0, left, "every carried identifier is under the new key");
    for value in &carried {
        let found: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pix.carried_identifier
             WHERE domain_id = $1 AND identifier_hmac = $2",
        )
        .bind(bsn_domain.id.uuid())
        .bind(new.sealer().hmac(bsn_domain.id.uuid(), value).as_slice())
        .fetch_one(new.pool())
        .await
        .unwrap();
        assert_eq!(1, found, "each BSN is found by the new key's HMAC");
    }
}
