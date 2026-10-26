// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The durable outbox over PostgreSQL 18 (#239), behind `FERROPIX_E2E`: an
//! XCPD asynchronous answer accepted with `202` is in the outbox before the
//! sender runs, so a sender started later, as after a restart, delivers it;
//! the drain waits for the message in flight; a message whose attempts are
//! spent is abandoned with its failure; the payload is sealed at rest; and
//! the settled messages past their retention are pruned through the one
//! function the server's role may call, never a pending one.
//!
//! Every name and identifier is synthetic.

use std::time::Duration;

use ferropix_server::config::section::outbox::{Outbox, Retention};
use ferropix_server::outbox::{self, Channels, Wake};
use ferropix_server::store::outbox::{self as store_outbox, Channel, Message};
use ferropix_server::xcpd::routes;
use ferropix_testkit::containers::e2e_enabled;
use http::StatusCode;
use secrecy::SecretString;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::pixm::{Harness, synthetic_seeds};
use super::xcpd::{
    FEMALE, SOAP, asynchronous, gateway_only, named, post, received, section, seeded,
};

/// The default `[outbox]` retention.
fn retention() -> Retention {
    Outbox::default().resolve().expect("the default retention")
}

/// One row of the outbox: attempts, delivered, abandoned, the last failure.
type Row = (i32, bool, bool, Option<String>);

/// The outbox's rows, oldest first.
async fn rows(harness: &Harness) -> Vec<Row> {
    sqlx::query_as(
        "SELECT attempts, delivered_at IS NOT NULL, abandoned_at IS NOT NULL, last_failure
         FROM pix.outbox ORDER BY created_at",
    )
    .fetch_all(harness.store.pool())
    .await
    .expect("the outbox reads")
}

/// Waits until the outbox's first row satisfies `settled`.
async fn settled(harness: &Harness, done: impl Fn(&Row) -> bool) -> Row {
    for _ in 0..200 {
        if let Some(row) = rows(harness).await.into_iter().next()
            && done(&row)
        {
            return row;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the outbox row did not settle: {:?}", rows(harness).await);
}

/// Sends Sanne Wolderink's asynchronous query to a gateway delivering to
/// `stub` with `attempts`, with no sender started, and returns the gateway's
/// outbox channels and wake.
async fn accepted(harness: &Harness, stub: &MockServer, attempts: u32) -> (Channels, Wake) {
    let mut xcpd = section(&[format!("{}/", stub.uri())]);
    xcpd.delivery_attempts = attempts;
    let wake = Wake::default();
    let gateway = gateway_only(
        harness,
        &harness.matching,
        xcpd.resolve().expect("resolves").expect("on"),
        &wake,
    );
    let channels = Channels {
        xcpd: Some(gateway.channel()),
    };
    let app = routes(gateway);
    let sanne = named("Sanne", "Wolderink", "19660817", FEMALE);
    let (status, body) = post(
        &app,
        SOAP,
        asynchronous(&format!("{}/replies", stub.uri()), &sanne),
    )
    .await;
    assert_eq!(StatusCode::ACCEPTED, status, "{body}");
    (channels, wake)
}

#[tokio::test]
async fn an_accepted_answer_waits_in_the_outbox_and_a_later_sender_delivers_it() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    seeded(&harness).await;
    let stub = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(202))
        .mount(&stub)
        .await;
    let (channels, wake) = accepted(&harness, &stub, 3).await;
    assert_eq!(
        vec![(0, false, false, None)],
        rows(&harness).await,
        "written, not sent"
    );
    let sealed: Vec<u8> = sqlx::query_scalar("SELECT payload_sealed FROM pix.outbox")
        .fetch_one(harness.store.pool())
        .await
        .expect("the payload reads");
    assert!(
        !String::from_utf8_lossy(&sealed).contains("Wolderink"),
        "the payload is sealed at rest"
    );
    assert!(
        stub.received_requests()
            .await
            .expect("recording")
            .is_empty()
    );
    let (_stop, stopped) = tokio::sync::watch::channel(false);
    let _sender = outbox::start(harness.store.clone(), channels, retention(), wake, stopped);
    let requests = received(&stub, 1).await;
    assert!(
        String::from_utf8_lossy(&requests[0].body).contains("Wolderink"),
        "the answer reaches the endpoint"
    );
    assert_eq!((1, true, false, None), settled(&harness, |row| row.1).await);
}

#[tokio::test]
async fn the_drain_waits_for_the_message_in_flight() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    seeded(&harness).await;
    let stub = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(202).set_delay(Duration::from_millis(600)))
        .mount(&stub)
        .await;
    let (channels, wake) = accepted(&harness, &stub, 3).await;
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let sender = outbox::start(harness.store.clone(), channels, retention(), wake, stopped);
    let _in_flight = received(&stub, 1).await;
    stop.send(true).expect("the sender listens");
    tokio::time::timeout(Duration::from_secs(5), sender)
        .await
        .expect("the sender stops within the drain")
        .expect("the sender's task ends");
    assert_eq!(
        vec![(1, true, false, None)],
        rows(&harness).await,
        "the message in flight was finished"
    );
}

#[tokio::test]
async fn a_message_whose_attempts_are_spent_is_abandoned_with_its_failure() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    seeded(&harness).await;
    let stub = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&stub)
        .await;
    let (channels, wake) = accepted(&harness, &stub, 2).await;
    let (_stop, stopped) = tokio::sync::watch::channel(false);
    let _sender = outbox::start(harness.store.clone(), channels, retention(), wake, stopped);
    let row = settled(&harness, |row| row.2).await;
    assert_eq!(
        (
            2,
            false,
            true,
            Some(String::from(
                "the endpoint answered 503 Service Unavailable"
            ))
        ),
        row
    );
    assert_eq!(2, received(&stub, 2).await.len());
}

/// Writes one message and returns its id.
async fn written(harness: &Harness) -> uuid::Uuid {
    let message = Message {
        channel: Channel::Xcpd,
        destination: String::from("https://ig.example.org/replies"),
        media_type: "application/soap+xml; charset=UTF-8",
        payload: SecretString::from(String::from("<answer>synthetic</answer>")),
        max_attempts: 3,
    };
    store_outbox::enqueue(harness.store.pool(), harness.store.sealer(), &message)
        .await
        .expect("written")
}

#[tokio::test]
async fn the_settled_messages_past_their_retention_are_pruned_and_a_pending_one_never_is() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let old_delivered = written(&harness).await;
    let old_abandoned = written(&harness).await;
    let fresh = written(&harness).await;
    let pending = written(&harness).await;
    for (id, column, days) in [
        (old_delivered, "delivered_at", 40),
        (old_abandoned, "abandoned_at", 40),
        (fresh, "delivered_at", 1),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE pix.outbox SET {column} = now() - make_interval(days => $2) WHERE id = $1"
        )))
        .bind(id)
        .bind(days)
        .execute(harness.store.pool())
        .await
        .expect("settled");
    }
    sqlx::query("UPDATE pix.outbox SET created_at = now() - interval '90 days' WHERE id = $1")
        .bind(pending)
        .execute(harness.store.pool())
        .await
        .expect("aged");
    let keep = Duration::from_hours(30 * 24);
    assert_eq!(
        1,
        store_outbox::prune(&harness.store, keep, 1)
            .await
            .expect("pruned"),
        "a batch of one"
    );
    assert_eq!(
        1,
        store_outbox::prune(&harness.store, keep, 10)
            .await
            .expect("pruned")
    );
    assert_eq!(
        0,
        store_outbox::prune(&harness.store, keep, 10)
            .await
            .expect("pruned")
    );
    let left: Vec<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM pix.outbox ORDER BY id")
        .fetch_all(harness.store.pool())
        .await
        .expect("the outbox reads");
    let mut want = vec![fresh, pending];
    want.sort();
    assert_eq!(want, left, "the fresh and the pending message stay");
}

#[tokio::test]
async fn the_servers_role_may_prune_the_outbox_but_not_delete_from_it() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let privileges: Vec<String> = sqlx::query_scalar(
        "SELECT a.privilege_type FROM pg_class c, aclexplode(c.relacl) a
         WHERE c.oid = 'pix.outbox'::regclass AND a.grantee = 'ferropix'::regrole
         ORDER BY 1",
    )
    .fetch_all(harness.store.pool())
    .await
    .expect("the grants read");
    assert_eq!(
        vec!["INSERT", "SELECT", "UPDATE"],
        privileges,
        "no DELETE on the outbox"
    );
    let execute: Vec<String> = sqlx::query_scalar(
        "SELECT a.privilege_type FROM pg_proc p, aclexplode(p.proacl) a
         WHERE p.oid = 'pix.prune_outbox(interval, integer)'::regprocedure
           AND a.grantee = 'ferropix'::regrole",
    )
    .fetch_all(harness.store.pool())
    .await
    .expect("the function's grants read");
    assert_eq!(
        vec!["EXECUTE"],
        execute,
        "the prune function is the scoped path"
    );
    let public: bool = sqlx::query_scalar(
        "SELECT has_function_privilege('public', 'pix.prune_outbox(interval, integer)', 'EXECUTE')",
    )
    .fetch_one(harness.store.pool())
    .await
    .expect("reads");
    assert!(!public, "no other role may prune");
}
