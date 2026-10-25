// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The durable outbox over PostgreSQL 18 (#239), behind `FERROPIX_E2E`: an
//! XCPD asynchronous answer accepted with `202` is in the outbox before the
//! sender runs, so a sender started later, as after a restart, delivers it;
//! the drain waits for the message in flight; a message whose attempts are
//! spent is abandoned with its failure; and the payload is sealed at rest.
//!
//! Every name and identifier is synthetic.

use std::time::Duration;

use ferropix_server::outbox::{self, Channels, Wake};
use ferropix_server::xcpd::routes;
use ferropix_testkit::containers::e2e_enabled;
use http::StatusCode;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::pixm::{Harness, synthetic_seeds};
use super::xcpd::{
    FEMALE, SOAP, asynchronous, gateway_only, named, post, received, section, seeded,
};

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
    let _sender = outbox::start(harness.store.clone(), channels, wake, stopped);
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
    let sender = outbox::start(harness.store.clone(), channels, wake, stopped);
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
    let _sender = outbox::start(harness.store.clone(), channels, wake, stopped);
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
