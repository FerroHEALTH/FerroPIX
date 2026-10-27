// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The store's readiness indicator: up while PostgreSQL answers, down once
//! it no longer does.

use ferrohealth_health::document::State;
use ferrohealth_health::registry::HealthIndicator;
use ferropix_server::store::indicator::{Database, NAME};
use ferropix_server::store::{Job, Store};
use ferropix_testkit::containers::{e2e_enabled, postgres};

use super::{app_role, fresh_key, seeds, settings};

#[tokio::test]
async fn the_database_indicator_is_up_while_the_store_answers_and_down_after() {
    if !e2e_enabled() {
        return;
    }
    let postgres = postgres().await.unwrap();
    let url = app_role(&postgres).await;
    let store = Store::start(&settings(&url, &fresh_key(), true), &seeds(), Job::Serve)
        .await
        .unwrap();
    let indicator = Database::new(store.clone());
    assert_eq!("database", NAME);
    assert_eq!(State::Up, indicator.check().await.state());
    store.close().await;
    let down = indicator.check().await;
    assert_eq!(State::Down, down.state(), "a closed pool answers nothing");
}
