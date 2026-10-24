// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The boot gate: `serve` refuses a superuser, a member of
//! `pg_read_all_data` and a role that can read a relation outside `pix`, and
//! starts for a role that reads `pix` alone.

use ferropix_server::store::{Job, Store, StoreError};
use ferropix_testkit::containers::{e2e_enabled, postgres};

use super::{APP_ROLE, admin, app_role, fresh_key, seeds, settings};

#[tokio::test]
async fn the_boot_gate_refuses_a_role_that_can_read_a_schema_beside_pix() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let key = fresh_key();
    let superuser = Store::start(&settings(postgres.url(), &key, true), &seeds(), Job::Serve)
        .await
        .expect_err("a superuser reads every schema");
    assert!(
        matches!(&superuser, StoreError::Superuser { role } if role == "ferropix"),
        "{superuser:?}"
    );

    let url = app_role(&postgres).await;
    Store::start(&settings(&url, &key, true), &seeds(), Job::Serve)
        .await
        .expect("a role that reads pix alone serves")
        .close()
        .await;

    let admin = admin(&postgres).await;
    for statement in [
        "CREATE SCHEMA ledger",
        "CREATE TABLE ledger.entry (id integer)",
        "GRANT USAGE ON SCHEMA ledger TO pix_app",
        "GRANT SELECT ON ledger.entry TO pix_app",
    ] {
        sqlx::query(statement).execute(&admin).await.unwrap();
    }
    let breach = Store::start(&settings(&url, &key, true), &seeds(), Job::Serve)
        .await
        .expect_err("a role that reads another schema is refused");
    assert!(
        matches!(&breach, StoreError::ReadsOtherSchema { role, relation, .. }
            if role == APP_ROLE && relation == "ledger.entry"),
        "{breach:?}"
    );

    for statement in [
        "REVOKE SELECT ON ledger.entry FROM pix_app",
        "GRANT pg_read_all_data TO pix_app",
    ] {
        sqlx::query(statement).execute(&admin).await.unwrap();
    }
    let reads_all = Store::start(&settings(&url, &key, true), &seeds(), Job::Serve)
        .await
        .expect_err("pg_read_all_data reads every schema");
    assert!(
        matches!(&reads_all, StoreError::ReadsAllData { role } if role == APP_ROLE),
        "{reads_all:?}"
    );

    Store::start(&settings(&url, &key, true), &seeds(), Job::Migrate)
        .await
        .expect("db migrate runs as the migrating role, which the gate does not hold")
        .close()
        .await;
    admin.close().await;
}
