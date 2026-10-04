// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The NVI Localization Service search against a stub service, with
//! localization records shaped after the IG's example
//! (`input/fsh/examples/gf-localization.fsh`) and synthetic values only: a
//! pseudonym and URAs that are visibly no real identifier.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the fixtures build FHIR JSON as values"
)]

mod answers;
mod contract;
mod hygiene;

use std::time::Duration;

use nl_generic_functions::identification::PseudoBsn;
use nl_generic_functions::nvi::NviClient;
use secrecy::SecretString;
use serde_json::{Value, json};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The pseudonym every search asks about.
pub(crate) const PATIENT: &str = "pbsn-synthetic-0001";

/// The search path under the stub's FHIR base.
pub(crate) const SEARCH: &str = "/fhir/DocumentReference";

/// The media type of every answer.
pub(crate) const FHIR_JSON: &str = "application/fhir+json";

/// A timeout no stub answer comes near.
pub(crate) const PROMPT: Duration = Duration::from_secs(5);

/// The patient the searches ask about.
pub(crate) fn patient() -> PseudoBsn {
    PseudoBsn::new(SecretString::from(PATIENT)).expect("a pseudonym")
}

/// A client of the stub service, following no redirect.
pub(crate) fn client(server: &MockServer) -> NviClient {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("an HTTP client");
    let base = Url::parse(&format!("{}/fhir", server.uri())).expect("a base URL");
    NviClient::new(base, http).expect("a client")
}

/// A client of a service nobody listens for.
///
/// A dropped `MockServer` goes back to wiremock's pool and keeps answering,
/// and a port bound and released can go to another test process, so neither
/// can stand for an unreachable service. Port 0 is never listened on, so a
/// connection to it fails at once.
pub(crate) fn unreachable_client() -> NviClient {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("an HTTP client");
    let base = Url::parse("http://127.0.0.1:0/fhir").expect("a base URL");
    NviClient::new(base, http).expect("a client")
}

/// A localization record of the IG's profile, the example's shape: `status`
/// `current`, LOINC `55188-7`, the subject by pseudonymised BSN and the
/// custodian by URA.
pub(crate) fn record(subject: &str, custodian: &str) -> Value {
    json!({
        "resourceType": "DocumentReference",
        "id": format!("loc-{custodian}"),
        "status": "current",
        "type": {
            "coding": [{
                "system": "http://loinc.org",
                "code": "55188-7",
                "display": "Patient data Document"
            }]
        },
        "subject": {
            "identifier": {
                "system": "http://fhir.nl/fhir/NamingSystem/pseudo-bsn",
                "value": subject
            }
        },
        "custodian": {
            "identifier": {
                "system": "http://fhir.nl/fhir/NamingSystem/ura",
                "value": custodian
            }
        },
        "content": [{
            "attachment": {
                "contentType": "application/json+fhir",
                "url": format!("https://{custodian}.example.org/fhirr4/Patient/synthetic-1")
            }
        }]
    })
}

/// A `searchset` Bundle of `resources`, with a `next` link when given.
pub(crate) fn searchset(resources: Vec<Value>, next: Option<&str>) -> Value {
    let entry: Vec<Value> = resources
        .into_iter()
        .map(|resource| json!({"resource": resource, "search": {"mode": "match"}}))
        .collect();
    let mut bundle = json!({
        "resourceType": "Bundle",
        "type": "searchset"
    });
    if !entry.is_empty() {
        // FHIR R4 JSON: an array is never empty, so a Bundle with no entry
        // leaves `entry` out.
        bundle["entry"] = Value::Array(entry);
    }
    if let Some(next) = next {
        bundle["link"] = json!([{"relation": "next", "url": next}]);
    }
    bundle
}

/// A stub service answering every search with `status`, `media` and `body`.
pub(crate) async fn service(status: u16, media: &str, body: &Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .respond_with(
            ResponseTemplate::new(status).set_body_raw(body.to_string().into_bytes(), media),
        )
        .mount(&server)
        .await;
    server
}
