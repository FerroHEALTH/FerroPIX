// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-78 against a stub Patient Demographics Supplier: the answers the
//! profile defines, the contract of the vendored capability statements and
//! response profile, and the hygiene of the demographic criteria.

mod answers;
mod contract;
mod hygiene;
mod paging;

use std::fmt::Write;
use std::path::PathBuf;
use std::time::Duration;

use ihe_iti::pdqm::PdqmClient;
use ihe_iti::pdqm::query::{PatientQuery, StringMatch};
use secrecy::SecretString;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The FHIR base the stub Supplier serves under.
pub(crate) const BASE: &str = "/fhir/";

/// The search path under [`BASE`].
pub(crate) const SEARCH: &str = "/fhir/Patient/_search";

/// The timeout of a request the stub answers at once.
pub(crate) const PROMPT: Duration = Duration::from_secs(5);

/// The FHIR JSON media type.
pub(crate) const FHIR_JSON: &str = "application/fhir+json";

/// The IG's example response Bundle.
pub(crate) const EXAMPLE_BUNDLE: &str =
    "example/Bundle-ex-QueryPatientResourceResponseMessage.json";

/// The IG's example Patients.
pub(crate) const EXAMPLE_PATIENT: &str = "example/Patient-ex-patient.json";
pub(crate) const EXAMPLE_MAIDEN_NAME: &str = "example/Patient-ex-patient-mothers-maiden-name.json";

/// A synthetic identifier domain under the example arc.
pub(crate) const DOMAIN: &str = "urn:oid:2.999.1";

/// A vendored file of the PDQm package, by its path under `package/`.
pub(crate) fn vendored(file: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../docs/specs/ihe-pdqm/package",
        file,
    ]
    .iter()
    .collect();
    std::fs::read_to_string(&path).expect("the vendored PDQm package (scripts/vendor/ihe-pdqm.sh)")
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("an HTTP client")
}

/// A client for `server`, built the way the module documentation asks: no
/// redirects.
pub(crate) fn client(server: &MockServer) -> PdqmClient {
    let base = Url::parse(&format!("{}{BASE}", server.uri())).expect("the stub base");
    PdqmClient::new(base, http()).expect("a client")
}

/// A client for a FHIR base nothing can listen on: port 0 on the loopback
/// interface.
///
/// A dropped `MockServer` goes back to wiremock's pool and keeps answering,
/// and a port bound and released can go to another test process, so neither
/// can stand for an unreachable Supplier. Binding port 0 picks another port,
/// so no listener ever holds it, and a connection to it fails at once.
pub(crate) fn unreachable_client() -> PdqmClient {
    let base = Url::parse(&format!("http://127.0.0.1:0{BASE}")).expect("a base");
    PdqmClient::new(base, http()).expect("a client")
}

/// The IG's example query: the family name `Schmidt`.
pub(crate) fn schmidt() -> PatientQuery {
    PatientQuery::new()
        .family(&SecretString::from("Schmidt"), StringMatch::StartsWith)
        .expect("a query")
}

/// A stub Supplier that answers every search with `status`, the media type
/// `media` and `body`.
pub(crate) async fn supplier(status: u16, media: &str, body: impl Into<String>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(ResponseTemplate::new(status).set_body_raw(body.into().into_bytes(), media))
        .mount(&server)
        .await;
    server
}

/// An `OperationOutcome` with one issue of `severity` and `code`, and `text` in
/// its `diagnostics`, which the client must never carry into an error.
pub(crate) fn outcome(severity: &str, code: &str, text: &str) -> String {
    format!(
        r#"{{"resourceType":"OperationOutcome","issue":[{{"severity":"{severity}","code":"{code}","diagnostics":"{text}"}}]}}"#
    )
}

/// A `searchset` Bundle with `total`, the JSON `entries` and the JSON `links`.
///
/// An empty list is left out, as FHIR JSON requires
/// (<http://hl7.org/fhir/R4/json.html#arrays>).
pub(crate) fn searchset(total: u32, entries: &[String], links: &[String]) -> String {
    let mut bundle = format!(r#"{{"resourceType":"Bundle","type":"searchset","total":{total}"#);
    if !links.is_empty() {
        write!(bundle, r#","link":[{}]"#, links.join(",")).expect("a String takes any text");
    }
    if !entries.is_empty() {
        write!(bundle, r#","entry":[{}]"#, entries.join(",")).expect("a String takes any text");
    }
    bundle.push('}');
    bundle
}

/// A Bundle entry holding `resource` at `full_url`, with the JSON `search`
/// element when there is one.
pub(crate) fn entry(full_url: &str, resource: &str, search: Option<&str>) -> String {
    match search {
        Some(search) => {
            format!(r#"{{"fullUrl":"{full_url}","resource":{resource},"search":{search}}}"#)
        }
        None => format!(r#"{{"fullUrl":"{full_url}","resource":{resource}}}"#),
    }
}
