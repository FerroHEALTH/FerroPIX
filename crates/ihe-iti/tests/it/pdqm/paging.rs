// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The incremental response: FHIR paging over the result set
//! (§2:3.78.4.2.2.4, <http://hl7.org/fhir/R4/http.html#paging>).

use ihe_iti::pdqm::error::{Malformation, PdqmError};
use ihe_iti::user::OnBehalfOf;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{FHIR_JSON, PROMPT, SEARCH, client, entry, schmidt, searchset};

fn patient(id: &str) -> String {
    entry(
        &format!("http://example.org/Patient/{id}"),
        &format!(r#"{{"resourceType":"Patient","id":"{id}"}}"#),
        None,
    )
}

fn next(url: &str) -> String {
    format!(r#"{{"relation":"next","url":"{url}"}}"#)
}

/// A Supplier with two pages, whose first page links to the URL `next_link`
/// makes of the server's own URI.
async fn two_pages(next_link: impl FnOnce(&str) -> String) -> MockServer {
    let server = MockServer::start().await;
    let link = next_link(&server.uri());
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            searchset(2, &[patient("first")], &[next(&link)]).into_bytes(),
            FHIR_JSON,
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fhir/Patient"))
        .and(query_param("page", "2"))
        .and(header("accept", FHIR_JSON))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            searchset(2, &[patient("second")], &[]).into_bytes(),
            FHIR_JSON,
        ))
        .mount(&server)
        .await;
    server
}

fn ids(page: &ihe_iti::pdqm::matches::SearchResult) -> Vec<Option<String>> {
    page.patients()
        .iter()
        .map(|matched| matched.patient().id.clone())
        .collect()
}

#[tokio::test]
async fn the_next_link_is_followed_on_the_suppliers_origin() {
    let server = two_pages(|uri| format!("{uri}/fhir/Patient?page=2")).await;
    let client = client(&server);
    let first = client
        .search(&schmidt(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("a first page");
    assert_eq!(ids(&first), [Some("first".to_owned())], "the first page");
    let page = first.next().expect("a next link");
    let second = client
        .next_page(page, &OnBehalfOf::System, PROMPT)
        .await
        .expect("a second page");
    assert_eq!(ids(&second), [Some("second".to_owned())], "the second page");
    assert!(second.next().is_none(), "the last page");
}

#[tokio::test]
async fn a_relative_next_link_resolves_against_the_fhir_base() {
    let server = two_pages(|_uri| "Patient?page=2".to_owned()).await;
    let client = client(&server);
    let first = client
        .search(&schmidt(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("a first page");
    let page = first.next().expect("a next link");
    let second = client
        .next_page(page, &OnBehalfOf::System, PROMPT)
        .await
        .expect("a second page");
    assert_eq!(ids(&second), [Some("second".to_owned())], "the second page");
}

#[tokio::test]
async fn a_next_link_to_another_origin_is_not_followed() {
    let server = two_pages(|_uri| "https://elsewhere.example/fhir/Patient?page=2".to_owned()).await;
    let client = client(&server);
    let first = client
        .search(&schmidt(), &OnBehalfOf::System, PROMPT)
        .await
        .expect("a first page");
    let page = first.next().expect("a next link");
    assert!(
        matches!(
            client.next_page(page, &OnBehalfOf::System, PROMPT).await,
            Err(PdqmError::ForeignPage)
        ),
        "the credentials stay with the Supplier"
    );
}

#[tokio::test]
async fn a_next_link_that_is_no_url_is_malformed() {
    let server = two_pages(|_uri| "http://[".to_owned()).await;
    assert!(
        matches!(
            client(&server)
                .search(&schmidt(), &OnBehalfOf::System, PROMPT)
                .await,
            Err(PdqmError::Malformed(Malformation::NextLink))
        ),
        "an unparsable next link"
    );
}
