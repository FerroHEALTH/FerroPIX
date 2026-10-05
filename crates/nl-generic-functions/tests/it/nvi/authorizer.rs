// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The authorizer of the Localization Service search: every request carries
//! the headers it makes for that request (the IG's GFI-005; RFC 9449 §7.1),
//! it is handed the URL without the query that holds the pseudonym, a
//! request it cannot authenticate is never sent, and a request it asks to
//! resend is sent at most twice (RFC 9449 §9).

use std::sync::{Arc, Mutex, PoisonError};

use http::header::AUTHORIZATION;
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use nl_generic_functions::nvi::authorizer::{Authorized, Authorizer, AuthorizerError, Retry};
use nl_generic_functions::nvi::error::NviError;
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{FHIR_JSON, PATIENT, PROMPT, SEARCH, client, patient, record, searchset};

/// What the test authorizer does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Behaviour {
    /// It makes headers, and asks for a resend after a `401`.
    Proves,
    /// It makes no headers.
    Fails,
}

/// An authorizer that numbers the headers it makes and records what it was
/// handed.
#[derive(Debug)]
struct Recording {
    behaviour: Behaviour,
    urls: Mutex<Vec<Url>>,
    answers: Mutex<Vec<StatusCode>>,
}

impl Recording {
    fn new(behaviour: Behaviour) -> Arc<Self> {
        Arc::new(Self {
            behaviour,
            urls: Mutex::new(Vec::new()),
            answers: Mutex::new(Vec::new()),
        })
    }

    fn urls(&self) -> Vec<Url> {
        self.urls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn answers(&self) -> Vec<StatusCode> {
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// The failure of the test authorizer.
#[derive(Debug, thiserror::Error)]
#[error("no synthetic credential")]
struct NoCredential;

impl Authorizer for Recording {
    fn authorize<'a>(&'a self, method: &'a Method, url: &'a Url) -> Authorized<'a> {
        Box::pin(async move {
            assert_eq!(Method::GET, *method, "the search is a GET");
            let count = {
                let mut urls = self.urls.lock().unwrap_or_else(PoisonError::into_inner);
                urls.push(url.clone());
                urls.len()
            };
            if self.behaviour == Behaviour::Fails {
                return Err(AuthorizerError::new(NoCredential));
            }
            let mut headers = HeaderMap::new();
            let mut token = HeaderValue::from_static("DPoP synthetic-token");
            token.set_sensitive(true);
            headers.insert(AUTHORIZATION, token);
            headers.insert(
                "dpop",
                HeaderValue::from_str(&format!("synthetic-proof-{count}"))
                    .map_err(AuthorizerError::new)?,
            );
            Ok(headers)
        })
    }

    fn answered(&self, _url: &Url, status: StatusCode, _headers: &HeaderMap) -> Retry {
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(status);
        if status == StatusCode::UNAUTHORIZED {
            Retry::Resend
        } else {
            Retry::Done
        }
    }
}

/// A service answering a search with `proof` with one record, and `401` to
/// any other.
async fn proven_service(proof: &str) -> MockServer {
    let server = MockServer::start().await;
    let body = searchset(vec![record(PATIENT, "ura-test-0001")], None);
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .and(header("authorization", "DPoP synthetic-token"))
        .and(header("dpop", proof))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(body.to_string().into_bytes(), FHIR_JSON),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .respond_with(ResponseTemplate::new(401))
        .with_priority(2)
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn every_request_carries_the_headers_its_authorizer_made() {
    let server = proven_service("synthetic-proof-1").await;
    let authorizer = Recording::new(Behaviour::Proves);
    let localization = client(&server)
        .with_authorizer(authorizer.clone())
        .localize(&patient(), PROMPT)
        .await
        .expect("a localization");
    assert_eq!(1, localization.custodians().len());
    assert_eq!(vec![StatusCode::OK], authorizer.answers());
}

#[tokio::test]
async fn the_authorizer_is_handed_the_url_without_the_pseudonym() {
    let server = proven_service("synthetic-proof-1").await;
    let authorizer = Recording::new(Behaviour::Proves);
    client(&server)
        .with_authorizer(authorizer.clone())
        .localize(&patient(), PROMPT)
        .await
        .expect("a localization");
    let expected = Url::parse(&format!("{}{SEARCH}", server.uri())).expect("a URL");
    assert_eq!(
        vec![expected],
        authorizer.urls(),
        "the htu of RFC 9449 §4.2"
    );
    for url in authorizer.urls() {
        assert!(
            !url.as_str().contains(PATIENT),
            "the authorizer saw the pseudonym"
        );
    }
}

#[tokio::test]
async fn a_request_the_authorizer_cannot_authenticate_is_never_sent() {
    let server = proven_service("synthetic-proof-1").await;
    let error = client(&server)
        .with_authorizer(Recording::new(Behaviour::Fails))
        .localize(&patient(), PROMPT)
        .await
        .expect_err("no credential");
    assert!(matches!(error, NviError::Unauthenticated(_)), "{error:?}");
    let requests = server.received_requests().await.expect("recorded requests");
    assert!(requests.is_empty(), "nothing was sent");
}

#[tokio::test]
async fn a_resend_the_authorizer_asks_for_carries_new_headers() {
    let server = proven_service("synthetic-proof-2").await;
    let authorizer = Recording::new(Behaviour::Proves);
    let localization = client(&server)
        .with_authorizer(authorizer.clone())
        .localize(&patient(), PROMPT)
        .await
        .expect("a localization on the resend");
    assert_eq!(1, localization.custodians().len());
    assert_eq!(
        vec![StatusCode::UNAUTHORIZED, StatusCode::OK],
        authorizer.answers()
    );
}

#[tokio::test]
async fn a_request_is_sent_at_most_twice() {
    let server = proven_service("synthetic-proof-3").await;
    let error = client(&server)
        .with_authorizer(Recording::new(Behaviour::Proves))
        .localize(&patient(), PROMPT)
        .await
        .expect_err("refused twice");
    assert!(
        matches!(error, NviError::Rejected { status } if status == StatusCode::UNAUTHORIZED),
        "{error:?}"
    );
    let requests = server.received_requests().await.expect("recorded requests");
    assert_eq!(2, requests.len(), "one resend at most");
}

#[tokio::test]
async fn the_client_renders_whether_it_has_an_authorizer_and_nothing_it_made() {
    let server = proven_service("synthetic-proof-1").await;
    let rendered = format!(
        "{:?}",
        client(&server).with_authorizer(Recording::new(Behaviour::Proves))
    );
    assert!(rendered.contains("authorizer: true"), "{rendered}");
    assert!(!rendered.contains("synthetic-token"), "{rendered}");
}
