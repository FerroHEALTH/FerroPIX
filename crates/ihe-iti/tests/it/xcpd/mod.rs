// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-55 against a stub Responding Gateway: the request the transaction
//! defines, the five cases of its response, and the hygiene of the patient
//! identifier (ITI TF-2 §3.55, Appendix O, Appendix V).
//!
//! The answers are the fixtures under `tests/fixtures/xcpd/`, shaped after
//! the examples of §3.55 with every identifier synthetic in the `2.999`
//! example arc. The stub fills in `RelatesTo` from the request's `MessageID`.

mod answers;
mod contract;
mod hygiene;

use std::path::PathBuf;
use std::time::Duration;

use ihe_iti::xcpd::XcpdClient;
use ihe_iti::xcpd::identifier::{Oid, PatientIdentifier};
use ihe_iti::xcpd::request::{DiscoveryQuery, RespondingGateway};
use secrecy::SecretString;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// The path the stub gateway serves.
pub(crate) const PATH: &str = "/RespondingGateway";

/// The synthetic patient identifier value the fixtures quote.
pub(crate) const PATIENT_VALUE: &str = "SYNTH-PATIENT-55";

/// The assigning authority of [`PATIENT_VALUE`].
pub(crate) const AUTHORITY: &str = "2.999.1";

/// The initiating gateway's device.
pub(crate) const SENDER: &str = "2.999.40.1";

/// The responding gateway's device.
pub(crate) const RECEIVER: &str = "2.999.50.1";

/// The SOAP 1.2 media type.
pub(crate) const SOAP_XML: &str = "application/soap+xml; charset=UTF-8";

/// The timeout of a request the stub answers at once.
pub(crate) const PROMPT: Duration = Duration::from_secs(5);

/// The fixture `name`.
pub(crate) fn fixture(name: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests/fixtures/xcpd", name]
        .iter()
        .collect();
    std::fs::read_to_string(&path).expect("a fixture under tests/fixtures/xcpd")
}

/// An answer whose `{relates_to}` becomes the request's `MessageID`.
pub(crate) struct Templated {
    status: u16,
    media: String,
    body: String,
}

impl Templated {
    /// An answer of `status` with `media` and `body`.
    pub(crate) fn new(status: u16, media: &str, body: impl Into<String>) -> Self {
        Self {
            status,
            media: media.to_owned(),
            body: body.into(),
        }
    }
}

impl Respond for Templated {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let sent = String::from_utf8_lossy(&request.body);
        let message = between(&sent, "<wsa:MessageID>", "</wsa:MessageID>").unwrap_or_default();
        let body = self.body.replace("{relates_to}", message);
        ResponseTemplate::new(self.status).set_body_raw(body.into_bytes(), &self.media)
    }
}

/// The text of `haystack` between `open` and the `close` after it.
pub(crate) fn between<'a>(haystack: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = haystack.find(open)? + open.len();
    let end = haystack.get(start..)?.find(close)? + start;
    haystack.get(start..end)
}

/// A stub gateway answering every request with `answer`.
pub(crate) async fn gateway(answer: Templated) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(PATH))
        .respond_with(answer)
        .mount(&server)
        .await;
    server
}

/// A stub gateway answering every request with the fixture `name`, as
/// SOAP 1.2 with `200`.
pub(crate) async fn answering(name: &str) -> MockServer {
    gateway(Templated::new(200, SOAP_XML, fixture(name))).await
}

/// The client, built the way the module documentation asks: no redirects.
pub(crate) fn client() -> XcpdClient {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("an HTTP client");
    XcpdClient::new(http)
}

/// The stub `server` as a responding gateway, over the development path,
/// since the stub speaks plain `http`.
pub(crate) fn responding(server: &MockServer) -> RespondingGateway {
    let endpoint = Url::parse(&format!("{}{PATH}", server.uri())).expect("the stub endpoint");
    RespondingGateway::unencrypted_for_development(endpoint, oid(RECEIVER))
        .expect("a responding gateway")
}

/// An OID.
pub(crate) fn oid(text: &str) -> Oid {
    Oid::new(text).expect("an OID")
}

/// The discovery of the synthetic patient.
pub(crate) fn query() -> DiscoveryQuery {
    let patient = PatientIdentifier::new(oid(AUTHORITY), SecretString::from(PATIENT_VALUE))
        .expect("a patient identifier");
    DiscoveryQuery::new(oid(SENDER), patient)
}

/// The body of the one request `server` received.
pub(crate) async fn sent(server: &MockServer) -> String {
    let requests = server.received_requests().await.expect("recording is on");
    assert_eq!(1, requests.len(), "one request per discovery");
    String::from_utf8(requests[0].body.clone()).expect("a UTF-8 request")
}
