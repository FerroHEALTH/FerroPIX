// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The store against PostgreSQL 18, behind `FERROPIX_E2E`.
//!
//! Each case starts its own container with the testkit, connects as a role
//! made for it that owns nothing but what it creates, and returns at once
//! when the gate is unset. Every identifier and name is synthetic, and every
//! key is generated when the case runs.

mod blocking;
mod cli;
mod decisions;
mod feed;
mod feeds;
mod gate;
mod hl7v2;
mod indicator;
mod leak;
mod operator;
mod operator_writes;
mod pdqm_match;
mod pdqm_search;
mod pixm;
mod reseal;
mod schema;
mod sealing;
mod supplier;

use base64::Engine as _;
use ferropix_server::config::secret::SecretUrl;
use ferropix_server::config::section::db::DbSettings;
use ferropix_server::config::section::domain::{DomainKind, DomainSeed};
use ferropix_server::store::Store;
use ferropix_server::store::record::{self, Channel, Demographics, Domain, RecordId};
use ferropix_server::store::seal::{IdentifierValue, SealingKey};
use ferropix_testkit::containers::Postgres;
use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN};
use sqlx::PgPool;

/// The BSN naming system, which the Dutch Generic Functions name (the
/// Identification page).
pub(crate) const BSN_SYSTEM: &str = "http://fhir.nl/fhir/NamingSystem/bsn";

/// The login role each case connects as: no superuser, `CREATE` on the
/// database and nothing else.
pub(crate) const APP_ROLE: &str = "pix_app";

/// Returns a sealing key generated for this run.
pub(crate) fn fresh_key() -> SealingKey {
    let mut bytes = [0_u8; 32];
    aws_lc_rs::rand::fill(&mut bytes).expect("random bytes");
    SealingKey::from_base64(&base64::engine::general_purpose::STANDARD.encode(bytes))
        .expect("a generated key reads")
}

/// Returns a superuser pool on the container.
pub(crate) async fn admin(postgres: &Postgres) -> PgPool {
    PgPool::connect(postgres.url())
        .await
        .expect("the superuser connects")
}

/// Creates [`APP_ROLE`] with `CREATE` on the database and returns its URL.
pub(crate) async fn app_role(postgres: &Postgres) -> String {
    let admin = admin(postgres).await;
    sqlx::query("CREATE ROLE pix_app LOGIN PASSWORD 'pix_app'")
        .execute(&admin)
        .await
        .expect("the role is created");
    sqlx::query("GRANT CREATE ON DATABASE ferropix TO pix_app")
        .execute(&admin)
        .await
        .expect("the role may create a schema");
    admin.close().await;
    postgres
        .url()
        .replacen("ferropix:ferropix@", &format!("{APP_ROLE}:{APP_ROLE}@"), 1)
}

/// Returns the `[db]` settings for `url` under `key`.
pub(crate) fn settings(url: &str, key: &SealingKey, migrate: bool) -> DbSettings {
    DbSettings {
        url: SecretUrl::new(url),
        pool_size: 4,
        migrate,
        data_key: key.clone(),
    }
}

/// The domains every case seeds: the BSN, sealed, and the synthetic
/// hospital's numbers, plain.
pub(crate) fn seeds() -> Vec<DomainSeed> {
    vec![
        DomainSeed {
            system: BSN_SYSTEM.to_owned(),
            name: String::from("BSN"),
            kind: DomainKind::Authoritative,
            source_client_id: String::from("synthetic-source"),
            sensitive: true,
        },
        DomainSeed {
            system: HOSPITAL_DOMAIN.to_owned(),
            name: String::from("Synthetic hospital"),
            kind: DomainKind::Local,
            source_client_id: String::from("synthetic-source"),
            sensitive: false,
        },
    ]
}

/// Returns the stored domain `system` names.
pub(crate) async fn domain(store: &Store, system: &str) -> Domain {
    record::domain(store.pool(), system)
        .await
        .expect("the domain reads")
        .expect("the domain is seeded")
}

/// Returns the synthetic BSN at `index` as an identifier value.
pub(crate) fn bsn(index: usize) -> IdentifierValue {
    IdentifierValue::new(
        ferropix_testkit::synthetic::bsn(index)
            .expect("a synthetic BSN")
            .digits(),
    )
    .expect("nine digits")
}

/// Returns demographics carrying the synthetic name at `index`.
pub(crate) fn demographics(index: usize) -> Demographics {
    let family = FAMILY_NAMES[index % FAMILY_NAMES.len()];
    let given = GIVEN_NAMES[index % GIVEN_NAMES.len()];
    Demographics::from_json(format!(
        r#"{{"name":[{{"family":"{family}","given":["{given}"]}}],"birthDate":"1970-01-01"}}"#
    ))
    .expect("the demographics are JSON")
}

/// Creates a record of `value` in `domain`, committed.
pub(crate) async fn create(
    store: &Store,
    domain: &Domain,
    value: &IdentifierValue,
    index: usize,
) -> RecordId {
    let mut transaction = store.pool().begin().await.expect("a transaction");
    let id = record::create(
        &mut transaction,
        store.sealer(),
        domain,
        value,
        &demographics(index),
        Channel::Fhir,
        Some("e2e"),
    )
    .await
    .expect("the record is created");
    transaction.commit().await.expect("the record commits");
    id
}
