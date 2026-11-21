// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pool without a ping on every checkout (#714): the sessions a warm
//! pool holds are cut by PostgreSQL, as a restart or a failover cuts them
//! (`pg_terminate_backend` sends the same `57P01` a fast shutdown does), and
//! an ITI-83 query after the cut either is answered or is refused cleanly
//! with `503`, at most once per pooled connection, and once a connection sat
//! idle past the probe its cut is found before a request gets it.

use std::time::Duration;

use ferropix_server::store::IDLE_PROBE;
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::fixtures::Fixture;
use ferropix_testkit::synthetic::HOSPITAL_DOMAIN;
use http::StatusCode;

use super::{Harness, fixture, hospital_identifiers, synthetic_seeds};
use crate::e2e::{APP_ROLE, admin};

/// The connections the harness's pool holds (`e2e::settings`).
const POOL: usize = 4;

/// Opens every connection of the pool with as many concurrent queries.
async fn warm(harness: &Harness, hospital: &str) {
    let (a, b, c, d) = tokio::join!(
        harness.pix_get(HOSPITAL_DOMAIN, hospital, &[]),
        harness.pix_get(HOSPITAL_DOMAIN, hospital, &[]),
        harness.pix_get(HOSPITAL_DOMAIN, hospital, &[]),
        harness.pix_get(HOSPITAL_DOMAIN, hospital, &[]),
    );
    for answer in [a, b, c, d] {
        assert_eq!(StatusCode::OK, answer.status, "{}", answer.body);
    }
}

/// Cuts every session of the server's role, and returns how many it cut.
async fn cut(harness: &Harness) -> i64 {
    let admin = admin(harness.postgres()).await;
    let cut: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (
             SELECT pg_terminate_backend(pid) FROM pg_stat_activity
             WHERE usename = $1 AND pid <> pg_backend_pid()
         ) AS cut",
    )
    .bind(APP_ROLE)
    .fetch_one(&admin)
    .await
    .expect("the sessions are cut");
    admin.close().await;
    cut
}

#[tokio::test]
async fn a_cut_session_is_refused_cleanly_once_and_found_by_the_idle_probe() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let (hospital, _bsn) = hospital_identifiers();
    let created = harness
        .feed(HOSPITAL_DOMAIN, &hospital, &fixture(Fixture::FeedHospital))
        .await;
    assert_eq!(StatusCode::CREATED, created.status, "{}", created.body);

    warm(&harness, &hospital).await;
    assert!(cut(&harness).await >= 1, "the warm pool holds sessions");
    let mut refused = 0;
    let mut last = StatusCode::OK;
    for _ in 0..=POOL {
        let answer = harness.pix_get(HOSPITAL_DOMAIN, &hospital, &[]).await;
        assert!(
            answer.status == StatusCode::OK || answer.status == StatusCode::SERVICE_UNAVAILABLE,
            "a query on a cut session is answered or refused with 503: {} {}",
            answer.status,
            answer.body
        );
        if answer.status == StatusCode::SERVICE_UNAVAILABLE {
            refused += 1;
        }
        last = answer.status;
    }
    assert!(
        refused <= POOL,
        "each cut connection refuses one query at most"
    );
    assert_eq!(
        StatusCode::OK,
        last,
        "a fresh connection answers after the cut ones"
    );

    warm(&harness, &hospital).await;
    assert!(cut(&harness).await >= 1, "the warm pool holds sessions");
    tokio::time::sleep(IDLE_PROBE + Duration::from_millis(200)).await;
    for _ in 0..POOL {
        let answer = harness.pix_get(HOSPITAL_DOMAIN, &hospital, &[]).await;
        assert_eq!(
            StatusCode::OK,
            answer.status,
            "the idle probe replaces a cut connection: {}",
            answer.body
        );
    }
}
