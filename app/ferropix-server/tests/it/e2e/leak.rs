// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! No identifier and no name reaches a log line, a `Debug` rendering or an
//! error message, through every path the store has: the start, a record
//! written, read back and refused, and a refused reseal.

use std::io::Write;
use std::sync::{Arc, Mutex};

use ferropix_server::chain;
use ferropix_server::store::record::{self, Channel, Demographics};
use ferropix_server::store::reseal;
use ferropix_server::store::seal::Sealer;
use ferropix_server::store::{Job, Store, StoreError};
use ferropix_testkit::containers::{e2e_enabled, postgres};
use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES};

use super::{BSN_SYSTEM, app_role, bsn, create, demographics, domain, fresh_key, seeds, settings};

/// A log sink the case reads back.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Capture {
    /// Returns a subscriber that writes every event, at every level, here.
    fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync + use<> {
        let sink = self.clone();
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || sink.clone())
            .finish()
    }
}

/// Every rendering of `error` a log line or a refusal can show.
fn renderings(error: &StoreError) -> String {
    format!("{error}\n{error:?}\n{}", chain(error))
}

#[tokio::test]
async fn no_identifier_or_name_reaches_a_log_a_debug_rendering_or_an_error() {
    if !e2e_enabled() {
        return;
    }
    let capture = Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());

    let postgres = postgres().await.unwrap();
    let url = app_role(&postgres).await;
    let store = Store::start(&settings(&url, &fresh_key(), true), &seeds(), Job::Migrate)
        .await
        .unwrap();
    let bsn_domain = domain(&store, BSN_SYSTEM).await;
    let value = bsn(0);
    let id = create(&store, &bsn_domain, &value, 0).await;
    let mut shown = vec![
        format!("{id:?} {bsn_domain:?} {value:?} {:?}", demographics(0)),
        format!("{store:?}"),
    ];

    let mut transaction = store.pool().begin().await.unwrap();
    let duplicate = record::create(
        &mut transaction,
        store.sealer(),
        &bsn_domain,
        &value,
        &demographics(0),
        Channel::Fhir,
        None,
    )
    .await
    .expect_err("one identifier is one record");
    transaction.rollback().await.unwrap();
    assert_eq!(
        Some("source_record_identifier_key"),
        duplicate
            .database()
            .and_then(|failure| failure.constraint())
    );
    shown.push(renderings(&duplicate));

    let names = format!(r#"["{}","{}"]"#, FAMILY_NAMES[1], GIVEN_NAMES[1]);
    let mut transaction = store.pool().begin().await.unwrap();
    let not_an_object = record::create(
        &mut transaction,
        store.sealer(),
        &bsn_domain,
        &bsn(1),
        &Demographics::from_json(names).unwrap(),
        Channel::V2,
        None,
    )
    .await
    .expect_err("demographics are an object");
    transaction.rollback().await.unwrap();
    assert_eq!(
        Some("source_record_version_demographics_check"),
        not_an_object
            .database()
            .and_then(|failure| failure.constraint())
    );
    shown.push(renderings(&not_an_object));

    let found = record::find(store.pool(), store.sealer(), &bsn_domain, &value)
        .await
        .unwrap();
    let opened = record::identifier(store.pool(), store.sealer(), id)
        .await
        .unwrap();
    assert_eq!((Some(id), &value), (found, &opened));
    shown.push(format!("{found:?} {opened:?}"));

    let stranger = Sealer::new(&fresh_key()).unwrap();
    let refused = reseal::run(&store, &stranger, reseal::BATCH)
        .await
        .expect_err("a key the store never held is no old key");
    shown.push(renderings(&refused));
    let unopened = record::identifier(store.pool(), &stranger, id)
        .await
        .expect_err("another key opens nothing");
    shown.push(renderings(&unopened));
    store.close().await;

    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("the domain seed is applied"),
        "the capture works"
    );
    shown.push(logs);
    let secrets = [
        value.expose().to_owned(),
        bsn(1).expose().to_owned(),
        FAMILY_NAMES[0].to_owned(),
        GIVEN_NAMES[0].to_owned(),
        FAMILY_NAMES[1].to_owned(),
        GIVEN_NAMES[1].to_owned(),
    ];
    for text in &shown {
        for secret in &secrets {
            assert!(
                !text.contains(secret.as_str()),
                "an identifier or a name leaked into: {text}"
            );
        }
    }
}

#[tokio::test]
async fn no_identifier_or_name_reaches_a_log_a_debug_rendering_or_an_error_through_the_feed() {
    if !e2e_enabled() {
        return;
    }
    let capture = Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());
    let started = super::feeds::start().await;
    let feeder = super::feeds::feeder(&started.store);
    let context = super::feeds::context();
    let shared = bsn(2).expose().to_owned();
    let (family, given) = (FAMILY_NAMES[2], GIVEN_NAMES[2]);
    let hospital = super::feeds::Patient::new(
        ferropix_testkit::synthetic::HOSPITAL_DOMAIN,
        "H-leak-1",
        family,
        given,
    )
    .carrying(BSN_SYSTEM, &shared);
    let mut laboratory = super::feeds::Patient::new(
        ferropix_testkit::synthetic::LABORATORY_DOMAIN,
        "L-leak-1",
        family,
        given,
    )
    .carrying(BSN_SYSTEM, &shared);
    laboratory.birth = String::from("1980-01-01");
    let mut shown = Vec::new();
    let revision = hospital.revision();
    let applied = feeder.revise(&revision, &context).await.unwrap();
    shown.push(format!("{applied:?} {revision:?} {feeder:?} {context:?}"));
    let conflicted = feeder
        .revise(&laboratory.revision(), &context)
        .await
        .unwrap();
    shown.push(format!("{conflicted:?}"));
    let merge = hospital.subsumed_by("H-leak-unknown");
    let refused = feeder
        .resolve_duplicate(&merge, &context)
        .await
        .expect_err("no such survivor");
    shown.push(format!("{refused} {refused:?} {}", chain(&refused)));
    let mut broken = shared.clone();
    broken.pop();
    broken.push(if shared.ends_with('0') { '1' } else { '0' });
    let not_a_bsn = feeder
        .revise(
            &super::feeds::Patient::new(BSN_SYSTEM, &broken, family, given).revision(),
            &context,
        )
        .await
        .expect_err("the eleven-proof fails");
    shown.push(format!("{not_a_bsn} {not_a_bsn:?} {}", chain(&not_a_bsn)));
    let mut connection = started.store.pool().acquire().await.unwrap();
    let found = ferropix_server::store::xref::lookup(
        &mut connection,
        started.store.sealer(),
        ferropix_testkit::synthetic::HOSPITAL_DOMAIN,
        "H-leak-1",
    )
    .await
    .unwrap();
    shown.push(format!(
        "{:?}",
        matches!(found, ferropix_server::store::xref::Lookup::Found(_))
    ));
    drop(connection);
    started.store.close().await;
    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("a feed is applied"), "the capture works");
    shown.push(logs);
    let secrets = [
        shared.clone(),
        broken,
        String::from("H-leak-1"),
        String::from("L-leak-1"),
        String::from("H-leak-unknown"),
        family.to_owned(),
        given.to_owned(),
    ];
    for text in &shown {
        for secret in &secrets {
            assert!(
                !text.contains(secret.as_str()),
                "an identifier or a name leaked into: {text}"
            );
        }
    }
}

#[tokio::test]
async fn no_identifier_or_name_reaches_a_log_a_span_or_a_refusal_through_the_pixm_routes() {
    if !e2e_enabled() {
        return;
    }
    let harness = super::pixm::Harness::start(super::pixm::synthetic_seeds()).await;
    let captured = crate::auth::Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_current_span(true)
        .with_span_list(true)
        .with_writer(captured.clone())
        .finish();
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let (hospital, stored) = super::pixm::feed_both(&harness).await;
    let unknown = bsn(7).expose().to_owned();
    let broken = String::from("999990001");
    let mut refusals = Vec::new();
    for (source, targets, expected) in [
        (stored.as_str(), &[][..], http::StatusCode::OK),
        (
            stored.as_str(),
            &["urn:oid:2.999.9.9"][..],
            http::StatusCode::FORBIDDEN,
        ),
        (unknown.as_str(), &[][..], http::StatusCode::NOT_FOUND),
        (broken.as_str(), &[][..], http::StatusCode::NOT_FOUND),
    ] {
        let got = harness.pix_get(BSN_SYSTEM, source, targets).await;
        assert_eq!(expected, got.status, "GET: {}", got.body);
        let posted = harness
            .pix_post(&super::pixm::pix_parameters(BSN_SYSTEM, source, targets))
            .await;
        assert_eq!(expected, posted.status, "POST: {}", posted.body);
        if expected != http::StatusCode::OK {
            refusals.push(got.body);
            refusals.push(posted.body);
        }
    }
    let refused_feed = harness
        .feed(
            BSN_SYSTEM,
            &broken,
            &ferropix_testkit::fixtures::Fixture::FeedLaboratory
                .document()
                .unwrap(),
        )
        .await;
    assert!(
        refused_feed.status.is_client_error(),
        "{}",
        refused_feed.body
    );
    refusals.push(refused_feed.body);
    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let requests = logs
        .lines()
        .filter(|line| line.contains("\"target\":\"ferropix_server::request\""))
        .count();
    assert_eq!(
        11, requests,
        "the capture holds one line per request: {logs}"
    );
    assert!(
        logs.contains("/fhir/Patient/$ihe-pix"),
        "the matched route is logged"
    );
    assert!(
        !logs.contains("sourceIdentifier"),
        "no query or body reaches the log"
    );
    let secrets = [
        stored.clone(),
        unknown,
        broken,
        hospital,
        String::from(super::pixm::LAB),
        FAMILY_NAMES[0].to_owned(),
        GIVEN_NAMES[0].to_owned(),
    ];
    for (place, text) in [("the log", logs), ("a refusal", refusals.join("\n"))] {
        for secret in &secrets {
            assert!(
                !text.contains(secret.as_str()),
                "{place} holds an identifier or a name: {text}"
            );
        }
    }
}
