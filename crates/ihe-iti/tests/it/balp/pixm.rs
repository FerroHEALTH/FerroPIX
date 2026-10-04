// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-83 audit record of an audited PIXm client (PIXm 3.1.0
//! §2:3.83.5.1.1): held to the Query Consumer audit profile and its example,
//! naming the patient by the source identifier, recorded whatever the
//! outcome, and failing the exchange when the recorder refuses it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ihe_iti::balp::Outcome;
use ihe_iti::pixm::error::PixmError;
use ihe_iti::recording::Late;
use secrecy::ExposeSecret as _;

use super::profile::{Kept, Refusing, Stalled, base64_decoded, holds_to, like, vendored, written};
use crate::pixm::{
    BLUE, FHIR_JSON, PROMPT, RED, RED_VALUE, client, manager, red_source, target,
    unreachable_client,
};

/// The vendored example answer of ITI-83 for the red patient and the blue
/// domain.
fn answer() -> String {
    crate::pixm::vendored("example/Parameters-pixm-response-mohralice-red-to-blue.json")
}

#[tokio::test]
async fn an_answered_query_is_recorded_as_the_consumer_audit_profile_fixes_it() {
    let server = manager(200, FHIR_JSON, answer()).await;
    let kept = Arc::new(Kept::default());
    let client = client(&server).audited(kept.clone());
    client
        .cross_reference(&red_source(), &[target(BLUE)], PROMPT)
        .await
        .expect("a cross-reference");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::Success);
    let record = written(&exchange);
    holds_to(
        &record,
        &vendored(
            "ihe-pixm",
            "package/StructureDefinition-IHE.PIXm.Query.Audit.Consumer.json",
        ),
    );
    like(
        &record,
        &vendored(
            "ihe-pixm",
            "package/example/AuditEvent-ex-auditPixmQuery-consumer.json",
        ),
        true,
    );
}

#[tokio::test]
async fn the_record_names_the_patient_and_the_request_toward_the_repository_only() {
    let server = manager(200, FHIR_JSON, answer()).await;
    let kept = Arc::new(Kept::default());
    let client = client(&server).audited(kept.clone());
    client
        .cross_reference(&red_source(), &[target(BLUE)], PROMPT)
        .await
        .expect("a cross-reference");
    let [exchange] = kept.taken().try_into().expect("one record");
    let record = written(&exchange);
    let patient = record["entity"]
        .as_array()
        .expect("entities")
        .iter()
        .find(|entity| entity["role"]["code"] == "1")
        .expect("a patient entity");
    assert_eq!(patient["what"]["identifier"]["system"], RED);
    assert_eq!(patient["what"]["identifier"]["value"], RED_VALUE);
    let query = record["entity"]
        .as_array()
        .expect("entities")
        .iter()
        .find(|entity| entity["role"]["code"] == "24")
        .expect("a query entity");
    let sent = base64_decoded(query["query"].as_str().expect("a query"));
    let asked = server.received_requests().await.expect("requests")[0]
        .url
        .clone();
    assert_eq!(
        sent,
        format!(
            "{}{}?{}",
            server.uri(),
            asked.path(),
            asked.query().unwrap_or_default()
        ),
        "the query entity is the request as sent"
    );
    assert_eq!(
        record["agent"][1]["who"]["display"],
        format!("{}/fhir/", server.uri()),
        "the Manager is named by its base"
    );
    assert!(
        !format!("{exchange:?}").contains(RED_VALUE),
        "Debug shows no identifier"
    );
}

#[tokio::test]
async fn an_unreachable_manager_is_recorded_as_a_serious_failure() {
    let kept = Arc::new(Kept::default());
    let client = unreachable_client().audited(kept.clone());
    let error = client
        .cross_reference(&red_source(), &[target(BLUE)], PROMPT)
        .await
        .expect_err("no Manager answers");
    assert!(matches!(error, PixmError::Transport(_)), "{error:?}");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::SeriousFailure);
}

#[tokio::test]
async fn a_refusal_is_recorded_as_a_minor_failure() {
    let server = manager(
        400,
        FHIR_JSON,
        crate::pixm::outcome("code-invalid", "unknown domain"),
    )
    .await;
    let kept = Arc::new(Kept::default());
    let client = client(&server).audited(kept.clone());
    client
        .cross_reference(&red_source(), &[target(BLUE)], PROMPT)
        .await
        .expect_err("a refusal");
    let [exchange] = kept.taken().try_into().expect("one record");
    assert_eq!(exchange.outcome, Outcome::MinorFailure);
}

#[tokio::test]
async fn an_answer_whose_record_is_refused_is_not_used() {
    let server = manager(200, FHIR_JSON, answer()).await;
    let client = client(&server).audited(Arc::new(Refusing));
    let error = client
        .cross_reference(&red_source(), &[target(BLUE)], PROMPT)
        .await
        .expect_err("the exchange fails closed");
    assert!(matches!(error, PixmError::Audit(_)), "{error:?}");
    assert!(
        !format!("{error:?} {error}").contains(RED_VALUE),
        "the error names no identifier"
    );
}

/// The time the exchanges below are given.
const BUDGET: Duration = Duration::from_millis(300);

/// The time a loaded host may add to any wait a test makes.
const SLACK: Duration = Duration::from_secs(3);

#[tokio::test]
async fn an_answer_whose_record_is_not_stored_within_the_exchange_s_time_is_not_used() {
    let server = manager(200, FHIR_JSON, answer()).await;
    let client = client(&server).audited(Arc::new(Stalled));
    let asked = Instant::now();
    let error = client
        .cross_reference(&red_source(), &[target(BLUE)], BUDGET)
        .await
        .expect_err("the exchange fails closed");
    assert!(asked.elapsed() < BUDGET + SLACK, "{:?}", asked.elapsed());
    let PixmError::Audit(audit) = &error else {
        panic!("an audit failure: {error:?}");
    };
    assert!(audit.0.is::<Late>(), "{audit:?}");
    assert!(!format!("{error:?} {error}").contains(RED_VALUE));
}

#[tokio::test]
async fn a_failed_exchange_whose_record_is_not_stored_in_time_keeps_its_own_failure() {
    let client = unreachable_client().audited(Arc::new(Stalled));
    let asked = Instant::now();
    let error = client
        .cross_reference(&red_source(), &[target(BLUE)], BUDGET)
        .await
        .expect_err("no Manager answers");
    assert!(asked.elapsed() < BUDGET + SLACK, "{:?}", asked.elapsed());
    assert!(matches!(error, PixmError::Transport(_)), "{error:?}");
}

#[test]
fn the_record_bytes_are_never_shown() {
    let exchange = ihe_iti::balp::Exchange {
        kind: ihe_iti::pixm::audit::QUERY_CONSUMER,
        recorded: jiff::Timestamp::UNIX_EPOCH,
        outcome: Outcome::Success,
        direction: ihe_iti::balp::Direction::Sent {
            server: ihe_iti::balp::Peer::server(
                &url::Url::parse("https://pix.example.org/fhir/").expect("a URL"),
            ),
        },
        entities: vec![ihe_iti::balp::Entity::Patient {
            system: RED.to_owned(),
            value: secrecy::SecretString::from(RED_VALUE),
        }],
    };
    let record = exchange.audit_event(&super::observer()).expect("a record");
    assert!(!format!("{record:?}").contains(RED_VALUE));
    assert!(
        String::from_utf8_lossy(record.into_bytes().expose_secret()).contains(RED_VALUE),
        "the bytes toward the repository carry it"
    );
}
