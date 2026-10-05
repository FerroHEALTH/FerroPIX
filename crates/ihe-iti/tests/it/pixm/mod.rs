// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-83 against a stub PIX Manager: the answers the profile defines, the
//! contract of the vendored `$ihe-pix` `OperationDefinition`, and the hygiene of
//! the source identifier.

mod answers;
mod contract;
mod hygiene;

use std::path::PathBuf;
use std::time::Duration;

use ihe_iti::pixm::identifier::{SourceIdentifier, TargetSystem};
use ihe_iti::pixm::{Invocation, PixmClient};
use secrecy::SecretString;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The FHIR base the stub Manager serves under.
pub(crate) const BASE: &str = "/fhir/";

/// The operation path under [`BASE`].
pub(crate) const OPERATION: &str = "/fhir/Patient/$ihe-pix";

/// The IG's example domains (the `MohrAlice` examples of PIXm 3.1.0).
pub(crate) const RED: &str = "urn:oid:1.3.6.1.4.1.21367.13.20.1000";
pub(crate) const GREEN: &str = "urn:oid:1.3.6.1.4.1.21367.13.20.2000";
pub(crate) const BLUE: &str = "urn:oid:1.3.6.1.4.1.21367.13.20.3000";

/// The IG's example source identifier value in the red domain.
pub(crate) const RED_VALUE: &str = "IHERED-994";

/// The timeout of a request the stub answers at once.
pub(crate) const PROMPT: Duration = Duration::from_secs(5);

/// The FHIR JSON media type.
pub(crate) const FHIR_JSON: &str = "application/fhir+json";

/// A vendored file of the PIXm package, by its path under `package/`.
pub(crate) fn vendored(file: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../docs/specs/ihe-pixm/package",
        file,
    ]
    .iter()
    .collect();
    std::fs::read_to_string(&path).expect("the vendored PIXm package (scripts/vendor/ihe-pixm.sh)")
}

/// A client for `server`, built the way the module documentation asks: no
/// redirects.
pub(crate) fn client(server: &MockServer) -> PixmClient {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("an HTTP client");
    let base = Url::parse(&format!("{}{BASE}", server.uri())).expect("the stub base");
    PixmClient::new(base, http).expect("a client")
}

/// A client for `server` that posts its query ([`Invocation::Post`]).
pub(crate) fn posting_client(server: &MockServer) -> PixmClient {
    client(server).invoked_by(Invocation::Post)
}

/// A stub Manager that answers every `$ihe-pix` `POST` with `status`, the
/// media type `media` and `body`, and nothing else.
pub(crate) async fn posting_manager(
    status: u16,
    media: &str,
    body: impl Into<String>,
) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(OPERATION))
        .respond_with(ResponseTemplate::new(status).set_body_raw(body.into().into_bytes(), media))
        .mount(&server)
        .await;
    server
}

/// A client for a FHIR base nothing can listen on: port 0 on the loopback
/// interface.
///
/// A dropped `MockServer` goes back to wiremock's pool and keeps answering,
/// and a port bound and released can go to another test process, so neither
/// can stand for an unreachable Manager. Binding port 0 picks another port,
/// so no listener ever holds it, and a connection to it fails at once.
pub(crate) fn unreachable_client() -> PixmClient {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("an HTTP client");
    let base = Url::parse(&format!("http://127.0.0.1:0{BASE}")).expect("a base");
    PixmClient::new(base, http).expect("a client")
}

/// The IG's example patient in the red domain.
pub(crate) fn red_source() -> SourceIdentifier {
    SourceIdentifier::new(RED, SecretString::from(RED_VALUE)).expect("a source identifier")
}

/// A target system.
pub(crate) fn target(system: &str) -> TargetSystem {
    TargetSystem::new(system).expect("a target system")
}

/// A stub Manager that answers every `$ihe-pix` call with `status`, the media
/// type `media` and `body`.
pub(crate) async fn manager(status: u16, media: &str, body: impl Into<String>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(OPERATION))
        .respond_with(ResponseTemplate::new(status).set_body_raw(body.into().into_bytes(), media))
        .mount(&server)
        .await;
    server
}

/// An `OperationOutcome` with one `error` issue of `code`, and `text` in its
/// `diagnostics`, which the client must never carry into an error.
pub(crate) fn outcome(code: &str, text: &str) -> String {
    format!(
        r#"{{"resourceType":"OperationOutcome","issue":[{{"severity":"error","code":"{code}","diagnostics":"{text}"}}]}}"#
    )
}
