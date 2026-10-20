// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The access token an authorizer incorporates in each ITI-83 request (IUA
//! ITI-72 §3.72.4.2): a fresh token after a `401`, never the refused one
//! again (§3.72.4.3), at most two sends, nothing sent without a token, and
//! the source identifier kept out of what the authorizer is handed.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use http::header::AUTHORIZATION;
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use ihe_iti::authorizer::{Authorized, Authorizer, AuthorizerError, Retry};
use ihe_iti::pixm::error::PixmError;
use ihe_iti::pixm::identifier::CrossReference;
use ihe_iti::user::OnBehalfOf;
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{FHIR_JSON, OPERATION, PROMPT, RED_VALUE, client, red_source, vendored};

/// The IG's answer for the red patient in every domain.
const ALL: &str = "example/Parameters-pixm-response-mohralice-red-all.json";

/// How the test authorizer behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A fresh token for every request, a new request after a `401`.
    Tokens,
    /// A new request after every answer.
    AlwaysResend,
    /// No token at all.
    Failing,
}

/// An authorizer that hands out `token-1`, `token-2`, … and records what it
/// was handed.
#[derive(Debug)]
struct Counting {
    mode: Mode,
    issued: AtomicUsize,
    urls: Mutex<Vec<String>>,
    statuses: Mutex<Vec<StatusCode>>,
}

impl Counting {
    fn new(mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            mode,
            issued: AtomicUsize::new(0),
            urls: Mutex::new(Vec::new()),
            statuses: Mutex::new(Vec::new()),
        })
    }

    fn urls(&self) -> Vec<String> {
        self.urls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn statuses(&self) -> Vec<StatusCode> {
        self.statuses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Authorizer for Counting {
    fn authorize<'a>(&'a self, _method: &'a Method, url: &'a Url) -> Authorized<'a> {
        self.urls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(url.to_string());
        let issued = self.issued.fetch_add(1, Ordering::SeqCst) + 1;
        let mode = self.mode;
        Box::pin(async move {
            if mode == Mode::Failing {
                return Err(AuthorizerError::new("synthetic: no token can be obtained"));
            }
            let mut value = HeaderValue::from_str(&format!("Bearer token-{issued}"))
                .map_err(AuthorizerError::new)?;
            value.set_sensitive(true);
            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, value);
            Ok(headers)
        })
    }

    fn answered(&self, _url: &Url, status: StatusCode, _headers: &HeaderMap) -> Retry {
        self.statuses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(status);
        if self.mode == Mode::AlwaysResend || status == StatusCode::UNAUTHORIZED {
            Retry::Resend
        } else {
            Retry::Done
        }
    }
}

/// A stub Manager that answers a request carrying `accepted` with the IG's
/// answer, and every other with `401`.
async fn manager_accepting(accepted: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(OPERATION))
        .and(header(AUTHORIZATION, accepted))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(vendored(ALL).into_bytes(), FHIR_JSON),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(OPERATION))
        .respond_with(ResponseTemplate::new(401))
        .with_priority(2)
        .mount(&server)
        .await;
    server
}

/// The number of requests `server` received.
async fn received(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .map_or(0, |requests| requests.len())
}

#[tokio::test]
async fn a_refused_token_is_replaced_and_the_request_sent_once_more() {
    let server = manager_accepting("Bearer token-2").await;
    let authorizer = Counting::new(Mode::Tokens);
    let answer = client(&server)
        .with_authorizer(authorizer.clone())
        .cross_reference(&red_source(), &[], &OnBehalfOf::System, PROMPT)
        .await;
    assert!(
        matches!(answer, Ok(CrossReference::Matched(_))),
        "{answer:?}"
    );
    assert_eq!(
        vec![StatusCode::UNAUTHORIZED, StatusCode::OK],
        authorizer.statuses()
    );
    assert_eq!(2, received(&server).await);
}

#[tokio::test]
async fn the_authorizer_never_sees_the_source_identifier() {
    let server = manager_accepting("Bearer token-1").await;
    let authorizer = Counting::new(Mode::Tokens);
    let answer = client(&server)
        .with_authorizer(authorizer.clone())
        .cross_reference(&red_source(), &[], &OnBehalfOf::System, PROMPT)
        .await;
    assert!(answer.is_ok(), "{answer:?}");
    let urls = authorizer.urls();
    assert_eq!(1, urls.len(), "{urls:?}");
    for url in urls {
        assert!(url.ends_with(OPERATION), "{url}");
        assert!(!url.contains(RED_VALUE) && !url.contains('?'), "{url}");
    }
}

#[tokio::test]
async fn a_request_is_sent_at_most_twice() {
    let server = manager_accepting("Bearer never-issued").await;
    let authorizer = Counting::new(Mode::AlwaysResend);
    let answer = client(&server)
        .with_authorizer(authorizer)
        .cross_reference(&red_source(), &[], &OnBehalfOf::System, PROMPT)
        .await;
    assert!(
        matches!(
            answer,
            Err(PixmError::Rejected {
                status: StatusCode::UNAUTHORIZED,
                ..
            })
        ),
        "{answer:?}"
    );
    assert_eq!(2, received(&server).await);
}

#[tokio::test]
async fn nothing_is_sent_without_a_token() {
    let server = manager_accepting("Bearer token-1").await;
    let answer = client(&server)
        .with_authorizer(Counting::new(Mode::Failing))
        .cross_reference(&red_source(), &[], &OnBehalfOf::System, PROMPT)
        .await;
    assert!(
        matches!(answer, Err(PixmError::Unauthenticated(_))),
        "{answer:?}"
    );
    assert_eq!(0, received(&server).await);
}
