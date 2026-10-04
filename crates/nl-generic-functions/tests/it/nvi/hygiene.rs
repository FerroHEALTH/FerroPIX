// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pseudonym reaches the Localization Service, which is the
//! transaction's purpose, and nothing else the client produces: no error's
//! `Display`, `Debug` or source chain, no answer's `Debug`, and no rendering
//! of the client.

use std::error::Error;
use std::fmt::Write;
use std::time::Duration;

use nl_generic_functions::identification::PseudoBsn;
use nl_generic_functions::nvi::NviClient;
use nl_generic_functions::nvi::error::NviError;
use secrecy::SecretString;
use serde_json::json;
use url::Url;
use wiremock::MockServer;

use super::{FHIR_JSON, PROMPT, client, record, searchset, service, unreachable_client};

/// A value that must appear nowhere but in the request to the service.
const SENTINEL: &str = "SENTINEL-pbsn-4711";

fn sentinel() -> PseudoBsn {
    PseudoBsn::new(SecretString::from(SENTINEL)).expect("a pseudonym")
}

/// The error, its `Debug`, and every error in its source chain, as text.
fn rendered(error: &NviError) -> String {
    let mut text = format!("{error} {error:?}");
    let mut cause: Option<&dyn Error> = error.source();
    while let Some(inner) = cause {
        write!(text, " {inner} {inner:?}").expect("a String takes any text");
        cause = inner.source();
    }
    text
}

async fn failure(server: &MockServer, timeout: Duration) -> NviError {
    client(server)
        .localize(&sentinel(), timeout)
        .await
        .expect_err("a failure")
}

#[tokio::test]
async fn the_pseudonym_reaches_the_service() {
    let server = service(200, FHIR_JSON, &searchset(Vec::new(), None)).await;
    client(&server)
        .localize(&sentinel(), PROMPT)
        .await
        .expect("a localization");
    let requests = server.received_requests().await.expect("recorded requests");
    let url = requests.first().expect("one request").url.to_string();
    assert!(url.contains(SENTINEL), "the search names the pseudonym");
}

#[tokio::test]
async fn no_error_carries_the_pseudonym() {
    let echoing = json!({
        "resourceType": "OperationOutcome",
        "issue": [{"severity": "error", "code": "processing", "diagnostics": SENTINEL}]
    });
    let rejected = service(500, FHIR_JSON, &echoing).await;
    let other_subject = service(
        200,
        FHIR_JSON,
        &searchset(vec![record("pbsn-synthetic-0099", "ura-test-0001")], None),
    )
    .await;
    let not_json = MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::any())
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_raw(format!("{{\"x\": \"{SENTINEL}"), FHIR_JSON),
        )
        .mount(&not_json)
        .await;
    let slow = MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::any())
        .respond_with(wiremock::ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&slow)
        .await;
    let gone = unreachable_client();
    let errors = [
        failure(&rejected, PROMPT).await,
        failure(&other_subject, PROMPT).await,
        failure(&not_json, PROMPT).await,
        failure(&slow, Duration::from_millis(100)).await,
        gone.localize(&sentinel(), PROMPT)
            .await
            .expect_err("a failure"),
    ];
    for error in &errors {
        let text = rendered(error);
        assert!(!text.contains(SENTINEL), "{text}");
        assert!(!text.contains("pseudo-bsn"), "no request URL: {text}");
    }
}

#[test]
fn no_rendering_of_the_client_or_the_pseudonym_shows_a_secret() {
    let base = Url::parse("https://Qz7user:Qz7password@nvi.example.org/fhir/").expect("a URL");
    let client = NviClient::new(base, reqwest::Client::new()).expect("a client");
    let text = format!("{client:?} {:?}", sentinel());
    assert!(!text.contains("Qz7password"), "{text}");
    assert!(!text.contains("Qz7user"), "{text}");
    assert!(!text.contains(SENTINEL), "{text}");
}
