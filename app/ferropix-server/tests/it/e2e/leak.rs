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
