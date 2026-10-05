// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The BSN reaches Mitz, which is the transaction's purpose, and nothing else
//! the client produces: no error's `Display`, `Debug` or source chain, no
//! question's or answer's `Debug`, and no rendering of the client.

use std::error::Error;
use std::fmt::Write;

use nl_generic_functions::mitz::error::MitzError;
use secrecy::SecretString;
use wiremock::MockServer;

use super::{
    CATEGORIES, HOLDER, PROMPT, SOAP_XML, answer, client, fault, mitz, question_about, result,
};

/// A value that must appear nowhere but in the request to Mitz.
const SENTINEL: &str = "SENTINEL-bsn-4711";

/// The error, its `Debug`, and every error in its source chain, as text.
fn rendered(error: &MitzError) -> String {
    let mut text = format!("{error} {error:?}");
    let mut cause: Option<&dyn Error> = error.source();
    while let Some(inner) = cause {
        write!(text, " {inner} {inner:?}").expect("a String takes any text");
        cause = inner.source();
    }
    text
}

async fn failure(server: &MockServer) -> MitzError {
    client(server)
        .ask(&question_about(SENTINEL, &CATEGORIES), PROMPT)
        .await
        .expect_err("no decision")
}

#[tokio::test]
async fn the_bsn_reaches_mitz() {
    let results = [
        result("Permit", CATEGORIES[0], SENTINEL, HOLDER),
        result("Deny", CATEGORIES[1], SENTINEL, HOLDER),
    ];
    let server = mitz(200, SOAP_XML, &answer(&results)).await;
    let decided = client(&server)
        .ask(&question_about(SENTINEL, &CATEGORIES), PROMPT)
        .await
        .expect("a decision per category");
    assert!(!format!("{decided:?}").contains(SENTINEL));
    let requests = server.received_requests().await.expect("recorded requests");
    let body = String::from_utf8_lossy(&requests.first().expect("one request").body).into_owned();
    assert!(body.contains(SENTINEL), "the question names the BSN");
}

#[tokio::test]
async fn no_failure_renders_the_bsn() {
    let cases = [
        mitz(
            500,
            SOAP_XML,
            &fault("s:Sender", &format!("unknown patient {SENTINEL}")),
        )
        .await,
        mitz(400, "text/plain", &format!("bad patient {SENTINEL}")).await,
        mitz(200, SOAP_XML, &format!("<s:Envelope>{SENTINEL}")).await,
        mitz(
            200,
            SOAP_XML,
            &answer(&[
                result("Indeterminate", CATEGORIES[0], SENTINEL, HOLDER),
                result("Permit", CATEGORIES[1], SENTINEL, HOLDER),
            ]),
        )
        .await,
        mitz(
            200,
            SOAP_XML,
            &answer(&[result(
                "Permit",
                CATEGORIES[0],
                "bsn-synthetic-0002",
                HOLDER,
            )]),
        )
        .await,
    ];
    for server in &cases {
        let text = rendered(&failure(server).await);
        assert!(!text.contains(SENTINEL), "{text}");
    }
}

#[test]
fn a_question_and_its_bsn_are_never_rendered() {
    let question = question_about(SENTINEL, &CATEGORIES);
    assert!(!format!("{question:?}").contains(SENTINEL));
    assert!(!format!("{:?}", question.patient()).contains(SENTINEL));
    let bsn = nl_generic_functions::mitz::question::Bsn::new(SecretString::from(SENTINEL))
        .expect("a BSN");
    assert_eq!("Bsn(***)", format!("{bsn:?}"));
}

#[tokio::test]
async fn the_client_renders_its_endpoint_only() {
    let server = MockServer::start().await;
    let rendered = format!("{:?}", client(&server));
    assert!(rendered.contains(&server.uri()), "{rendered}");
}
