// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The source identifier reaches the PIX Manager, which is the transaction's
//! purpose, and nothing else the client produces: no error's `Display`,
//! `Debug` or source chain, and no answer's `Debug`.

use std::error::Error;
use std::fmt::Write;
use std::time::Duration;

use ihe_iti::pixm::PixmClient;
use ihe_iti::pixm::error::PixmError;
use ihe_iti::pixm::identifier::{CrossReference, SourceIdentifier};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use secrecy::SecretString;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{
    BLUE, FHIR_JSON, OPERATION, PROMPT, RED, client, manager, outcome, target, unreachable_client,
};

/// A value that must appear nowhere but in the request to the Manager.
const SENTINEL: &str = "SENTINEL-4711";

/// The user name, the password and the bearer token a client is built with,
/// which no rendering of the client shows.
const USER: &str = "Qz7user";
const PASSWORD: &str = "Qz7password";
const TOKEN: &str = "Qz7token";

fn source() -> SourceIdentifier {
    SourceIdentifier::new(RED, SecretString::from(SENTINEL)).expect("a source identifier")
}

/// The error, its `Debug`, and every error in its source chain, as text.
fn rendered(error: &PixmError) -> String {
    let mut text = format!("{error} {error:?}");
    let mut cause: Option<&dyn Error> = error.source();
    while let Some(inner) = cause {
        write!(text, " {inner} {inner:?}").expect("a String takes any text");
        cause = inner.source();
    }
    text
}

async fn failure(server: &MockServer, timeout: Duration) -> PixmError {
    client(server)
        .cross_reference(&source(), &[target(BLUE)], timeout)
        .await
        .expect_err("a failure")
}

#[tokio::test]
async fn the_identifier_reaches_the_manager() {
    let server = manager(200, FHIR_JSON, r#"{"resourceType":"Parameters"}"#).await;
    client(&server)
        .cross_reference(&source(), &[target(BLUE)], PROMPT)
        .await
        .expect("an answer");
    let requests = server.received_requests().await.expect("recorded requests");
    let sent = requests
        .first()
        .expect("one request")
        .url
        .query_pairs()
        .any(|(name, value)| name == "sourceIdentifier" && value.ends_with(SENTINEL));
    assert!(
        sent,
        "the source identifier is the request's input (§2:3.83.4.1.2.1)"
    );
}

#[tokio::test]
async fn no_failure_carries_the_identifier() {
    // Each Manager quotes the identifier where a careless client would copy it:
    // the diagnostics of an OperationOutcome, a parameter name, a code.
    let bodies = [
        (400, outcome("code-invalid", SENTINEL)),
        (403, outcome("code-invalid", SENTINEL)),
        (404, outcome("processing", SENTINEL)),
        (500, outcome(SENTINEL, SENTINEL)),
        (
            200,
            format!(
                r#"{{"resourceType":"Parameters","parameter":[{{"name":"{SENTINEL}","valueString":"x"}}]}}"#
            ),
        ),
        (
            200,
            format!(
                r#"{{"resourceType":"Parameters","parameter":[{{"name":"targetIdentifier","valueIdentifier":{{"system":"urn:oid:2.999.9","value":"{SENTINEL}"}}}}]}}"#
            ),
        ),
        (
            200,
            format!(r#"{{"resourceType":"Parameters","{SENTINEL}":1}}"#),
        ),
        (200, format!("{{\"{SENTINEL}\"")),
    ];
    for (status, body) in bodies {
        let server = manager(status, FHIR_JSON, body).await;
        let shown = rendered(&failure(&server, PROMPT).await);
        assert!(
            !shown.contains(SENTINEL),
            "a {status} answer's error carries the identifier"
        );
    }
}

#[tokio::test]
async fn a_timeout_or_transport_failure_carries_no_request_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(OPERATION))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(10)))
        .mount(&server)
        .await;
    let shown = rendered(&failure(&server, Duration::from_millis(200)).await);
    assert!(
        !shown.contains(SENTINEL),
        "the timeout carries the identifier"
    );
    let error = unreachable_client()
        .cross_reference(&source(), &[target(BLUE)], PROMPT)
        .await
        .expect_err("no Manager");
    let shown = rendered(&error);
    assert!(
        !shown.contains(SENTINEL) && !shown.contains("ihe-pix"),
        "the transport error carries the request URL"
    );
}

#[tokio::test]
async fn an_answer_shows_no_identifier_value() {
    let body = format!(
        r#"{{"resourceType":"Parameters","parameter":[{{"name":"targetIdentifier","valueIdentifier":{{"system":"{BLUE}","value":"{SENTINEL}-BLUE"}}}},{{"name":"targetId","valueReference":{{"reference":"Patient/{SENTINEL}"}}}}]}}"#
    );
    let server = manager(200, FHIR_JSON, body).await;
    let answer = client(&server)
        .cross_reference(&source(), &[target(BLUE)], PROMPT)
        .await
        .expect("an answer");
    assert!(
        matches!(answer, CrossReference::Matched(_)),
        "a cross-reference"
    );
    assert!(
        !format!("{answer:?}").contains(SENTINEL),
        "the Debug of an answer shows an identifier value"
    );
}

#[test]
fn a_client_shows_no_credential() {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {TOKEN}")).expect("a header value"),
    );
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .default_headers(headers)
        .build()
        .expect("an HTTP client");
    let base = Url::parse(&format!("https://{USER}:{PASSWORD}@pix.example.org/fhir/"))
        .expect("a base with userinfo");
    let client = PixmClient::new(base, http).expect("a client");
    for shown in [format!("{client:?}"), format!("{client:#?}")] {
        for credential in [USER, PASSWORD, TOKEN] {
            assert!(!shown.contains(credential), "{shown}");
        }
        assert!(
            shown.contains("https://***@pix.example.org/fhir/Patient/$ihe-pix"),
            "{shown}"
        );
    }
}
