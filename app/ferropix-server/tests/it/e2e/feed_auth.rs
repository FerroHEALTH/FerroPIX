// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-93 sender's client authentication (#655), behind `FERROPIX_E2E`
//! (PMIR 1.6.0 §2:3.93.5):
//! - a delivery to an endpoint under a `[[pmir.feed.consumers]]` prefix
//!   carries the Consumer's IUA token, renewed once when the Consumer
//!   answers `401`, in place of any `Authorization` header the
//!   subscription names, and an endpoint under no prefix gets no token;
//! - a Consumer that keeps refusing the credential with `403` is tried
//!   within the bounds, then its subscription is set to `error` naming the
//!   refusal;
//! - a Consumer that requires a client certificate takes the message when
//!   its table names the client identity, and refuses the handshake when it
//!   names none.
//!
//! Every name and identifier is synthetic, and the certificates are
//! generated when the case runs.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the subscription request is edited as a JSON value"
)]

use std::time::Duration;

use ferrohealth_secret::secret_url::SecretUrl;
use ferrohealth_smart::client::outbound::OutboundClient;
use ferrohealth_smart::config::ServerTls;
use ferrohealth_smart::config::credentials::{CredentialsSection, OauthSection};
use ferrohealth_smart::config::endpoint::{EndpointSection, RetrySection};
use ferropix_server::config::section::outbox::Outbox;
use ferropix_server::config::section::profiles::FeedConsumer;
use ferropix_server::feed::Feeder;
use ferropix_server::feed::master::MasterChange;
use ferropix_server::outbox::{self, Backoff, Channels, PmirChannel};
use ferropix_server::store::Store;
use ferropix_server::store::outbox::Wake;
use ferropix_server::store::subscription;
use ferropix_testkit::containers::e2e_enabled;
use ihe_iti::pmir::server::subscription::RequestedSubscription;
use ihe_iti::server::format::Format;
use secrecy::SecretString;
use serde_json::Value;
use uuid::Uuid;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::feed_sender::{
    FIRST, MASTER, context, feeding, patient, person, request, rows, until, with_master,
};
use super::pixm::Harness;
use super::xcpd_mtls::{Pki, connections, endpoint, pki, written};

/// The Authorization header a subscription names, which the Consumer's
/// credential replaces.
const NAMED: &str = "Bearer subscription-named";

/// Stores an active subscription to `endpoint` naming the [`NAMED`]
/// Authorization header.
async fn subscribe(store: &Store, endpoint: &str) -> Uuid {
    let mut resource = request("Patient", endpoint, "requested");
    resource["channel"]["header"] =
        Value::Array(vec![Value::String(format!("Authorization: {NAMED}"))]);
    let requested = RequestedSubscription::read(Format::Json, resource.to_string().as_bytes())
        .expect("a PMIR Subscription");
    let created = subscription::create(
        &mut store.pool().acquire().await.expect("a connection"),
        &requested,
    )
    .await
    .expect("stored");
    Uuid::parse_str(created.id.as_deref().expect("an id")).expect("a UUID")
}

/// The Consumer whose prefix is `base`, its table edited by `edit`.
fn consumer(base: &str, edit: impl FnOnce(&mut EndpointSection)) -> FeedConsumer {
    let mut table = EndpointSection {
        url: SecretUrl::new(base),
        timeout: ferrohealth_config::units::Duration::from_secs(5),
        retry: RetrySection {
            max_attempts: 1,
            ..RetrySection::default()
        },
        ..EndpointSection::default()
    };
    edit(&mut table);
    FeedConsumer {
        base: url::Url::parse(base).expect("a URL"),
        client: OutboundClient::build(&table, "pmir.feed.consumers", &ServerTls)
            .expect("the consumer's client"),
    }
}

/// The client credentials grant of the authorization server `tokens`.
fn grant(tokens: &MockServer) -> CredentialsSection {
    CredentialsSection {
        oauth: Some(OauthSection {
            token_endpoint: SecretUrl::new(format!("{}/token", tokens.uri())),
            client_id: String::from("synthetic-registry"),
            client_secret: Some(SecretString::from("synthetic-secret")),
            ..OauthSection::default()
        }),
        ..CredentialsSection::default()
    }
}

/// An authorization server that issues `token-1`, then `token-2`.
async fn authorization_server() -> MockServer {
    let server = MockServer::start().await;
    for (priority, token, times) in [(1, "token-1", Some(1)), (2, "token-2", None)] {
        let mock = Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": token,
                "token_type": "Bearer",
                "expires_in": 300
            })));
        let mock = match times {
            Some(times) => mock.up_to_n_times(times),
            None => mock,
        };
        mock.with_priority(priority).mount(&server).await;
    }
    server
}

/// Starts the sender over `store` delivering ITI-93 through `consumers`
/// with short waits.
fn sender(
    store: &Store,
    wake: &Wake,
    consumers: Vec<FeedConsumer>,
) -> tokio::sync::watch::Sender<bool> {
    let channels = Channels {
        xcpd: None,
        pmir: Some(PmirChannel {
            client: reqwest::Client::new(),
            consumers: consumers.into(),
            backoff: Backoff {
                initial: Duration::from_millis(20),
                max: Duration::from_millis(40),
            },
        }),
        hl7v2: None,
    };
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let retention = Outbox::default().resolve().expect("the default retention");
    let _sender = outbox::start(store.clone(), channels, retention, wake.clone(), stopped);
    stop
}

/// Creates the first person, which every subscription matches.
async fn create(harness: &Harness, store: &Store) {
    super::feeds::Fed::with(Feeder::new(store.clone(), &harness.matching), store)
        .master(
            MasterChange::Create,
            MASTER,
            person(FIRST),
            &patient(FIRST, "Anna"),
            &context(),
        )
        .await
        .expect("created");
}

/// The `Authorization` values `server` received on `route`, in order.
async fn authorizations(server: &MockServer, route: &str) -> Vec<Option<String>> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.url.path() == route)
        .map(|request| {
            request
                .headers
                .get(http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        })
        .collect()
}

#[tokio::test]
async fn a_consumer_gets_its_iua_token_renewed_once_and_an_endpoint_under_no_prefix_none() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(with_master()).await;
    let wake = Wake::default();
    let store = feeding(&harness, 3, &wake);
    let tokens = authorization_server().await;
    let stub = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/pmir"))
        .and(header("authorization", "Bearer token-2"))
        .respond_with(ResponseTemplate::new(200))
        .with_priority(1)
        .mount(&stub)
        .await;
    Mock::given(method("POST"))
        .and(path("/pmir"))
        .respond_with(ResponseTemplate::new(401))
        .with_priority(2)
        .mount(&stub)
        .await;
    Mock::given(method("POST"))
        .and(path("/other"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&stub)
        .await;
    subscribe(&store, &format!("{}/pmir", stub.uri())).await;
    subscribe(&store, &format!("{}/other", stub.uri())).await;
    create(&harness, &store).await;

    let consumers = vec![consumer(&format!("{}/pmir", stub.uri()), |table| {
        table.credentials = grant(&tokens);
    })];
    let _stop = sender(&store, &wake, consumers);
    until(&store, |rows| {
        rows.len() == 2 && rows.iter().all(|row| row.2)
    })
    .await;
    assert_eq!(
        vec![
            Some(String::from("Bearer token-1")),
            Some(String::from("Bearer token-2"))
        ],
        authorizations(&stub, "/pmir").await,
        "the refused token is renewed once, and the subscription's header never sent"
    );
    assert_eq!(
        vec![Some(NAMED.to_owned())],
        authorizations(&stub, "/other").await,
        "an endpoint under no consumer's prefix gets no token, only its own header"
    );
}

#[tokio::test]
async fn a_consumer_refusing_the_credential_is_retried_within_the_bounds_and_set_to_error() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(with_master()).await;
    let wake = Wake::default();
    let store = feeding(&harness, 2, &wake);
    let tokens = authorization_server().await;
    let stub = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/pmir"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&stub)
        .await;
    let id = subscribe(&store, &format!("{}/pmir", stub.uri())).await;
    create(&harness, &store).await;

    let consumers = vec![consumer(&format!("{}/pmir", stub.uri()), |table| {
        table.credentials = grant(&tokens);
    })];
    let _stop = sender(&store, &wake, consumers);
    until(&store, |rows| rows.first().is_some_and(|row| row.3)).await;
    assert_eq!(
        2,
        authorizations(&stub, "/pmir").await.len(),
        "two attempts within the bounds"
    );
    let read = subscription::read(&store, id)
        .await
        .expect("reads")
        .expect("there");
    assert_eq!(Some("error"), read.status.value.as_deref());
    assert!(
        read.error
            .as_ref()
            .and_then(|error| error.value.as_deref())
            .is_some_and(|error| error.contains("refused the credential") && error.contains("403")),
        "the error names the refusal: {:?}",
        read.error
    );
}

/// The Consumer at `base` trusting the authority of `pki`, presenting the
/// client identity when `identity` holds.
fn tls_consumer(base: &str, pki: &Pki, dir: &tempfile::TempDir, identity: bool) -> FeedConsumer {
    let roots = written(dir, "authority.pem", &pki.authority);
    let client = identity.then(|| {
        written(
            dir,
            "client.pem",
            &format!("{}\n{}\n", pki.client.0, pki.client.1),
        )
    });
    consumer(base, |table| {
        table.tls.trust_roots_file = Some(roots);
        table.tls.client_identity_file = client;
    })
}

#[tokio::test]
async fn a_consumer_that_requires_a_client_certificate_takes_the_message_only_with_one() {
    if !e2e_enabled() {
        return;
    }
    for identity in [true, false] {
        let harness = Harness::start(with_master()).await;
        let wake = Wake::default();
        let store = feeding(&harness, 1, &wake);
        let pki = pki();
        let (address, seen) = endpoint(&pki).await;
        let base = format!("https://localhost:{}/pmir", address.port());
        subscribe(&store, &base).await;
        create(&harness, &store).await;
        let dir = tempfile::tempdir().expect("a temporary directory");
        let _stop = sender(
            &store,
            &wake,
            vec![tls_consumer(&base, &pki, &dir, identity)],
        );
        let (handshakes, requests) = connections(&seen, 1).await;
        until(&store, |rows| {
            rows.first().is_some_and(|row| row.2 || row.3)
        })
        .await;
        let settled = rows(&store).await;
        if identity {
            assert_eq!(vec![true], handshakes, "the handshake completes");
            assert!(requests[0].starts_with("POST /pmir"), "{}", requests[0]);
            assert!(settled[0].2, "delivered: {settled:?}");
        } else {
            assert_eq!(
                vec![false],
                handshakes,
                "no client certificate, no handshake"
            );
            assert!(requests.is_empty(), "no request reached the consumer");
            assert!(settled[0].3, "abandoned after its one attempt: {settled:?}");
        }
    }
}
