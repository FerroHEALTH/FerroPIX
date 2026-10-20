// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The demographic criteria reach the Supplier, which is the transaction's
//! purpose, in the request body and nowhere else the client produces: not the
//! request URL, no error's `Display`, `Debug` or source chain, and no
//! `Debug` of a query, a result, a match or a page link.

use std::error::Error;
use std::fmt::Write;
use std::time::Duration;

use ihe_iti::pdqm::PdqmClient;
use ihe_iti::pdqm::error::PdqmError;
use ihe_iti::pdqm::query::{DatePrefix, PatientQuery, StringMatch};
use ihe_iti::user::OnBehalfOf;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use secrecy::SecretString;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{
    DOMAIN, FHIR_JSON, PROMPT, SEARCH, client, entry, outcome, searchset, supplier,
    unreachable_client,
};
use crate::timing;

/// A value that must appear nowhere but in the request to the Supplier.
const SENTINEL: &str = "SENTINEL-4711";

/// A birth date that must appear nowhere but in the request to the Supplier.
const BIRTH_DATE: &str = "1947-11-03";

/// The user name, the password and the bearer token a client is built with,
/// which no rendering of the client shows.
const USER: &str = "Qz7user";
const PASSWORD: &str = "Qz7password";
const TOKEN: &str = "Qz7token";

fn query() -> PatientQuery {
    let sentinel = SecretString::from(SENTINEL);
    PatientQuery::new()
        .family(&sentinel, StringMatch::Exact)
        .and_then(|query| query.given(&sentinel, StringMatch::StartsWith))
        .and_then(|query| query.identifier(Some(DOMAIN), &sentinel))
        .and_then(|query| query.telecom(&sentinel))
        .and_then(|query| query.address_postalcode(&sentinel, StringMatch::Exact))
        .and_then(|query| query.mothers_maiden_name(&sentinel, StringMatch::Exact))
        .and_then(|query| query.birthdate(DatePrefix::Eq, &SecretString::from(BIRTH_DATE)))
        .expect("a query")
}

fn shows_a_value(text: &str) -> bool {
    text.contains(SENTINEL) || text.contains(BIRTH_DATE)
}

/// The error, its `Debug`, and every error in its source chain, as text.
fn rendered(error: &PdqmError) -> String {
    let mut text = format!("{error} {error:?}");
    let mut cause: Option<&dyn Error> = error.source();
    while let Some(inner) = cause {
        write!(text, " {inner} {inner:?}").expect("a String takes any text");
        cause = inner.source();
    }
    text
}

#[tokio::test]
async fn the_criteria_reach_the_supplier_in_the_body_only() {
    let server = supplier(200, FHIR_JSON, searchset(0, &[], &[])).await;
    client(&server)
        .search(&query(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("an answer");
    let requests = server.received_requests().await.expect("recorded requests");
    let request = requests.first().expect("one request");
    let body = String::from_utf8_lossy(&request.body);
    assert!(
        body.contains(SENTINEL) && body.contains(BIRTH_DATE),
        "the criteria are the request's input (§2:3.78.4.1.2.1)"
    );
    assert!(
        !shows_a_value(request.url.as_str()),
        "the request URL carries a criterion"
    );
    for (name, value) in &request.headers {
        let raw = value.as_bytes();
        let carries = [SENTINEL, BIRTH_DATE].iter().any(|shown| {
            raw.windows(shown.len())
                .any(|window| window == shown.as_bytes())
        });
        assert!(
            !shows_a_value(name.as_str()) && !carries,
            "the {name} header carries a criterion"
        );
    }
}

#[test]
fn a_query_shows_no_value() {
    let shown = format!("{:?}", query());
    assert!(!shows_a_value(&shown), "the Debug of a query: {shown}");
}

#[tokio::test]
async fn no_failure_carries_a_value() {
    // Each Supplier quotes the criteria where a careless client would copy
    // them: the diagnostics of an OperationOutcome, a code, an unknown member,
    // a malformed decimal, a parse snippet.
    let bodies = [
        (404, outcome("warning", "not-found", SENTINEL)),
        (406, outcome("error", "not-supported", SENTINEL)),
        (500, outcome("error", SENTINEL, SENTINEL)),
        (
            200,
            format!(r#"{{"resourceType":"Bundle","type":"searchset","total":0,"{SENTINEL}":1}}"#),
        ),
        (
            200,
            searchset(
                1,
                &[entry(
                    &format!("http://example.org/Patient/{SENTINEL}"),
                    &format!(r#"{{"resourceType":"Patient","birthDate":"{SENTINEL}"}}"#),
                    None,
                )],
                &[],
            ),
        ),
        (
            200,
            searchset(
                1,
                &[entry(
                    "http://example.org/Patient/p",
                    r#"{"resourceType":"Patient"}"#,
                    Some(&format!(
                        r#"{{"extension":[{{"url":"http://hl7.org/fhir/StructureDefinition/match-grade","valueCode":"{SENTINEL}"}}]}}"#
                    )),
                )],
                &[],
            ),
        ),
        (200, format!("{{\"{SENTINEL}\"")),
    ];
    for (status, body) in bodies {
        let server = supplier(status, FHIR_JSON, body).await;
        let error = client(&server)
            .search(&query(), &OnBehalfOf::System, PROMPT)
            .await
            .expect_err("a failure");
        let shown = rendered(&error);
        assert!(
            !shows_a_value(&shown),
            "a {status} answer's error carries a value: {shown}"
        );
    }
}

#[tokio::test]
async fn a_timeout_or_transport_failure_carries_no_url() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(ResponseTemplate::new(200).set_delay(timing::SILENT))
        .mount(&server)
        .await;
    let client = client(&server);
    let limit = Duration::from_millis(200);
    let error = timing::bounded(limit, client.search(&query(), &OnBehalfOf::System, limit))
        .await
        .expect_err("a timeout");
    assert!(
        !shows_a_value(&rendered(&error)),
        "the timeout carries a value"
    );
    let error = unreachable_client()
        .search(&query(), &OnBehalfOf::System, PROMPT)
        .await
        .expect_err("no Supplier");
    let shown = rendered(&error);
    assert!(
        !shows_a_value(&shown) && !shown.contains("_search"),
        "the transport error carries the request URL: {shown}"
    );
}

#[tokio::test]
async fn a_page_link_and_its_failure_carry_no_value() {
    let server = MockServer::start().await;
    let link = format!(
        r#"{{"relation":"next","url":"{}/fhir/Patient?family={SENTINEL}"}}"#,
        server.uri()
    );
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(searchset(0, &[], &[link]).into_bytes(), FHIR_JSON),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fhir/Patient"))
        .respond_with(ResponseTemplate::new(200).set_delay(timing::SILENT))
        .mount(&server)
        .await;
    let client = client(&server);
    let first = client
        .search(&query(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("a first page");
    let page = first.next().expect("a next link");
    assert!(
        !shows_a_value(&format!("{first:?} {page:?}")),
        "the Debug of a result or a page link"
    );
    assert_eq!("Page(***)", format!("{page:?}"), "the family's placeholder");
    let limit = Duration::from_millis(200);
    let error = timing::bounded(limit, client.next_page(page, &OnBehalfOf::System, limit))
        .await
        .expect_err("a timeout");
    assert!(
        !shows_a_value(&rendered(&error)),
        "the page's timeout carries its URL"
    );
}

#[tokio::test]
async fn a_match_shows_no_demographics() {
    let patient = format!(
        r#"{{"resourceType":"Patient","id":"p","name":[{{"family":"{SENTINEL}"}}],"birthDate":"{BIRTH_DATE}"}}"#
    );
    let body = searchset(
        1,
        &[entry(
            &format!("http://example.org/Patient/{SENTINEL}"),
            &patient,
            Some(r#"{"mode":"match","score":0.8}"#),
        )],
        &[],
    );
    let server = supplier(200, FHIR_JSON, body).await;
    let found = client(&server)
        .search(&query(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("a result set");
    assert_eq!(found.patients().len(), 1, "one match");
    let shown = format!("{found:?}");
    assert!(
        !shows_a_value(&shown),
        "the Debug of a result shows a demographic: {shown}"
    );
    assert!(
        shown.contains(r#"full_url: "***", patient: "***""#),
        "the family's placeholder: {shown}"
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
    let base = Url::parse(&format!("https://{USER}:{PASSWORD}@pdq.example.org/fhir/"))
        .expect("a base with userinfo");
    let client = PdqmClient::new(base, http).expect("a client");
    for shown in [format!("{client:?}"), format!("{client:#?}")] {
        for credential in [USER, PASSWORD, TOKEN] {
            assert!(!shown.contains(credential), "{shown}");
        }
        assert!(
            shown.contains("https://***@pdq.example.org/fhir/Patient/_search"),
            "{shown}"
        );
    }
}
