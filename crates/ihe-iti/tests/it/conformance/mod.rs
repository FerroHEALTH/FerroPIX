// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The conformance corpora: every example the vendored PIXm, PDQm and PMIR
//! packages ship, read and written through `ihe-iti`, one verdict per
//! example, ratcheted against the pass lists under `conformance/`.
//!
//! A corpus test checks each example in a task of its own and records the
//! verdicts through `ferropix_conformance`.
//!
//! No specification governs the instrument: our own design, the model of
//! FerroBRIDGE's corpus tests.

mod audit;
mod pdqm;
mod pixm;
mod pmir;

use std::path::{Path, PathBuf};

use fhir_types::codec::Object;
use ihe_iti::server::format::Format;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The workspace root, relative to this crate's manifest.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The examples of the vendored package `corpus` (a directory under
/// `docs/specs/`), as paths under the package, sorted.
pub(crate) fn examples(corpus: &str) -> Vec<String> {
    let directory = Path::new(ROOT)
        .join("docs/specs")
        .join(corpus)
        .join("package/example");
    let mut found: Vec<String> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| {
            let name = entry.expect("a directory entry").file_name();
            format!("example/{}", name.to_str().expect("a UTF-8 file name"))
        })
        .filter(|id| {
            Path::new(id)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        })
        .collect();
    found.sort();
    found
}

/// Whether the example `id` is an audit record, which the audit corpus reads.
pub(crate) fn is_audit(id: &str) -> bool {
    id.starts_with("example/AuditEvent-")
}

/// The vendored example `id` of the package `corpus`, as JSON text.
pub(crate) fn example(corpus: &str, id: &str) -> String {
    let path: PathBuf = [ROOT, "docs/specs", corpus, "package", id].iter().collect();
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The vendored example `id` of the package `corpus` in `format`: as vendored
/// for JSON, converted for XML.
pub(crate) fn encoded(corpus: &str, id: &str, format: Format) -> Vec<u8> {
    let text = example(corpus, id);
    if format == Format::Xml {
        let value: fhir_types::codec::Value = serde_json::from_str(&text).expect("JSON");
        let object: &Object = value.as_object().expect("a resource");
        return fhir_types::xml::to_xml(&fhir_types::r4::schema::SCHEMAS, object)
            .expect("an XML form")
            .into_bytes();
    }
    text.into_bytes()
}

/// A stub PMIR Registry that creates `Subscription/s-1`, answers its read
/// with `subscription`, and deletes it.
pub(crate) async fn registry(subscription: String) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/fhir/Subscription"))
        .respond_with(ResponseTemplate::new(201).insert_header("location", "Subscription/s-1"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fhir/Subscription/s-1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(subscription.into_bytes(), "application/fhir+json"),
        )
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/fhir/Subscription/s-1"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    server
}
