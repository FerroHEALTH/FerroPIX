// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! PMIR 1.6.0: the ITI-93 feed read against the vendored message examples
//! and profiles, the messages it refuses, the ITI-94 subscription against a
//! stub Patient Identity Registry, and the hygiene of the identifiers both
//! carry.

mod contract;
mod feed;
mod hygiene;
mod subscription;

use std::path::PathBuf;
use std::time::Duration;

use ihe_iti::pmir::PmirSubscriber;
use url::Url;
use wiremock::MockServer;

/// The FHIR base the stub Registry serves under.
pub(crate) const BASE: &str = "/fhir/";

/// The FHIR JSON media type.
pub(crate) const FHIR_JSON: &str = "application/fhir+json";

/// The timeout of a request the stub answers at once.
pub(crate) const PROMPT: Duration = Duration::from_secs(5);

/// A synthetic assigning authority inside the `urn:oid:2.999` example arc.
pub(crate) const DOMAIN: &str = "urn:oid:2.999.1.47";

/// A vendored file of the PMIR package, by its path under `package/`.
pub(crate) fn vendored(file: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../docs/specs/ihe-pmir/package",
        file,
    ]
    .iter()
    .collect();
    std::fs::read_to_string(&path).expect("the vendored PMIR package (scripts/vendor/ihe-pmir.sh)")
}

/// A subscriber for `server`, built the way the module documentation asks:
/// no redirects.
pub(crate) fn subscriber(server: &MockServer) -> PmirSubscriber {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("an HTTP client");
    let base = Url::parse(&format!("{}{BASE}", server.uri())).expect("the stub base");
    PmirSubscriber::new(base, http).expect("a subscriber")
}

/// A message Bundle whose history holds `entries`, each a history entry's
/// JSON, under a header whose fields are `header`.
pub(crate) fn message_with(header: &str, entries: &[String]) -> String {
    format!(
        r#"{{"resourceType":"Bundle","type":"message","entry":[{{"fullUrl":"https://pmir.example.org/fhir/MessageHeader/m-1","resource":{{"resourceType":"MessageHeader",{header}}}}},{{"fullUrl":"https://pmir.example.org/fhir/Bundle/h-1","resource":{{"resourceType":"Bundle","id":"h-1","type":"history","entry":[{}]}}}}]}}"#,
        entries.join(",")
    )
}

/// The fields of a header that holds to the PMIR `MessageHeader` profile.
pub(crate) const HEADER: &str = r#""id":"m-1","eventUri":"urn:ihe:iti:pmir:2019:patient-feed","destination":[{"endpoint":"https://gateway.example.org/pmir/feed"}],"source":{"endpoint":"https://pmir.example.org/fhir"},"focus":[{"reference":"Bundle/h-1"}]"#;

/// A message whose history holds `entries`, under a header that holds to the
/// profile.
pub(crate) fn message(entries: &[String]) -> String {
    message_with(HEADER, entries)
}

/// A history entry of `method` on `url`, carrying `resource` when given,
/// answered `status`.
pub(crate) fn entry(method: &str, url: &str, resource: Option<&str>, status: &str) -> String {
    let resource =
        resource.map_or_else(String::new, |resource| format!(r#""resource":{resource},"#));
    format!(
        r#"{{"fullUrl":"https://pmir.example.org/fhir/{url}",{resource}"request":{{"method":"{method}","url":"{url}"}},"response":{{"status":"{status}"}}}}"#
    )
}

/// A Patient `id` with one identifier `value` in [`DOMAIN`] and a name,
/// with `extra` fields.
pub(crate) fn patient(id: &str, value: &str, extra: &str) -> String {
    format!(
        r#"{{"resourceType":"Patient","id":"{id}","identifier":[{{"system":"{DOMAIN}","value":"{value}"}}],"name":[{{"family":"Synthetic"}}]{extra}}}"#
    )
}

/// The merged Patient `id`, replaced by `surviving`, with one identifier
/// `value` in [`DOMAIN`].
pub(crate) fn merged(id: &str, value: &str, surviving: &str) -> String {
    patient(
        id,
        value,
        &format!(
            r#","active":false,"link":[{{"other":{{"reference":"Patient/{surviving}"}},"type":"replaced-by"}}]"#
        ),
    )
}
