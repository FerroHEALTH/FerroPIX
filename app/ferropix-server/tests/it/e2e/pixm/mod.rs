// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The PIXm Manager over PostgreSQL 18 (#66, #68, #52), behind
//! `FERROPIX_E2E`: ITI-104 fed through the router as a Source sends it, and
//! ITI-83 asked by `GET` and by `POST`, in JSON and in XML.
//!
//! The feeds are the testkit's synthetic fixtures and the vendored PIXm
//! 3.1.0 package's examples (the Mohr Alice set); every token is signed by
//! the in-memory authorization server of the guard's tests.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the answers are read as JSON values"
)]

mod iti104;
mod iti83;
mod vendored;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use ferrohealth_health::registry::Registry;
use ferrohealth_server::health::Health;
use ferrohealth_telemetry::request_log::RequestLog;
use ferropix_server::audit::Audit;
use ferropix_server::auth::Smart;
use ferropix_server::config::section::domain::{DomainKind, DomainSeed};
use ferropix_server::fhir::{self, Backend, Mounts};
use ferropix_server::http::router;
use ferropix_server::store::{Job, Store};
use ferropix_testkit::containers::{Postgres, postgres};
use ferropix_testkit::fixtures::Fixture;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN};
use fhir_types::codec::{Object, Value as Document};
use http::{HeaderMap, Method, Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt as _;

use super::{BSN_SYSTEM, app_role, fresh_key};
use crate::auth::{Issuer, bearer};
use crate::support::Secrets;

/// The client every seeded domain names as its Source.
pub(crate) const SOURCE: &str = "east-lab";

/// A client the issuer vouches for that is the Source of no domain.
pub(crate) const STRANGER: &str = "west-lab";

/// The Red, Green and Blue domains of the PIXm 3.1.0 examples.
const RED: &str = "urn:oid:1.3.6.1.4.1.21367.13.20.1000";
const GREEN: &str = "urn:oid:1.3.6.1.4.1.21367.13.20.2000";
const BLUE: &str = "urn:oid:1.3.6.1.4.1.21367.13.20.3000";

/// Returns a domain seed fed by [`SOURCE`].
fn seed(system: &str, kind: DomainKind, sensitive: bool) -> DomainSeed {
    DomainSeed {
        system: system.to_owned(),
        name: system.to_owned(),
        kind,
        source_client_id: SOURCE.to_owned(),
        sensitive,
    }
}

/// The synthetic domains: the BSN, authoritative and sealed, and the three
/// local ones.
pub(crate) fn synthetic_seeds() -> Vec<DomainSeed> {
    vec![
        seed(BSN_SYSTEM, DomainKind::Authoritative, true),
        seed(HOSPITAL_DOMAIN, DomainKind::Local, false),
        seed(LABORATORY_DOMAIN, DomainKind::Local, false),
        seed(PRACTICE_DOMAIN, DomainKind::Local, false),
    ]
}

/// A running Manager over its own PostgreSQL.
pub(crate) struct Harness {
    /// The router, every switch on.
    pub app: Router,
    /// The authorization server.
    pub issuer: Issuer,
    /// The audit spool's parent directory.
    pub spool: tempfile::TempDir,
    /// The store the router serves, which the HL7 v2 face shares.
    pub store: Store,
    /// The `[matching]` section the router scores with.
    pub matching: ferropix_server::config::section::matching::Matching,
    /// How the router answers a merged person.
    pub deprecated: ferropix_server::config::section::profiles::Deprecated,
    /// The public base URL the router is reached at.
    pub public_base_url: url::Url,
    /// The `[audit]` section the router records under.
    pub audit_settings: ferropix_server::config::section::audit::AuditSettings,
    _postgres: Postgres,
    _secrets: Secrets,
}

impl Harness {
    /// Starts PostgreSQL, the store with `seeds`, and the router.
    pub(crate) async fn start(seeds: Vec<DomainSeed>) -> Self {
        Self::start_with(seeds, |_matching| {}).await
    }

    /// Starts as [`Harness::start`] does, with the `[matching]` section
    /// `tweak` changes.
    pub(crate) async fn start_with(
        seeds: Vec<DomainSeed>,
        tweak: impl FnOnce(&mut ferropix_server::config::section::matching::Matching),
    ) -> Self {
        let postgres = postgres().await.expect("PostgreSQL starts");
        let url = app_role(&postgres).await;
        let store = Store::start(
            &super::settings(&url, &fresh_key(), true),
            &seeds,
            Job::Migrate,
        )
        .await
        .expect("the store starts");
        let issuer = Issuer::start().await;
        let secrets = Secrets::new();
        let mut settings = crate::auth::settings(&issuer, &secrets);
        settings.domains = seeds;
        tweak(&mut settings.matching);
        let smart = Arc::new(
            Smart::discover(settings.auth, &settings.domains)
                .await
                .expect("the authorization server is read"),
        );
        let spool = tempfile::tempdir().expect("a temporary directory");
        let mut audit_settings = settings.audit.clone();
        audit_settings.spool_dir = spool.path().join("spool");
        let audit =
            Audit::open(&audit_settings, &settings.public_base_url).expect("the audit spool opens");
        let health = Registry::default();
        let lifecycle = crate::support::serving();
        let routes = fhir::routes(
            fhir::Address {
                base: settings.server.base_path.as_str(),
                public_base_url: &settings.public_base_url,
            },
            Mounts::all(),
            &smart,
            &health,
            &lifecycle,
            &audit,
            &Backend {
                store: store.clone(),
                matching: settings.matching.clone(),
                deprecated: settings.pdqm.deprecated,
                configured: ferropix_server::operator::Configured {
                    provenance: ferrohealth_config::provenance::Provenance::default(),
                    senders: vec![ferropix_server::config::section::hl7v2::Sender {
                        application: String::from("ADT"),
                        facility: String::from("EAST"),
                        domain: HOSPITAL_DOMAIN.to_owned(),
                    }],
                },
                development: false,
            },
        );
        Self {
            app: router(
                &settings.server,
                Health::new(health, lifecycle),
                Some(routes),
                RequestLog::default(),
                None,
            ),
            issuer,
            spool,
            store,
            matching: settings.matching.clone(),
            deprecated: settings.pdqm.deprecated,
            public_base_url: settings.public_base_url.clone(),
            audit_settings: settings.audit.clone(),
            _postgres: postgres,
            _secrets: secrets,
        }
    }

    /// A Consumer's token.
    pub(crate) fn consumer(&self) -> String {
        bearer(&self.issuer.token("synthetic-consumer", "system/Patient.rs"))
    }

    /// A data steward's token.
    pub(crate) fn steward(&self) -> String {
        bearer(&self.issuer.token("synthetic-console", "__ferropix.steward"))
    }

    /// An administrator's token.
    pub(crate) fn admin(&self) -> String {
        bearer(&self.issuer.token("synthetic-console", "__ferropix.admin"))
    }

    /// The Source's token.
    pub(crate) fn source(&self) -> String {
        bearer(&self.issuer.token(SOURCE, "system/Patient.cud"))
    }

    /// The text of every record the spool holds.
    pub(crate) fn audit_texts(&self) -> Vec<String> {
        std::fs::read_dir(self.spool.path().join("spool"))
            .expect("the spool reads")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .map(|entry| std::fs::read_to_string(entry.path()).expect("a record reads"))
            .collect()
    }

    /// The records the spool holds.
    pub(crate) fn records(&self) -> usize {
        std::fs::read_dir(self.spool.path().join("spool"))
            .expect("the spool reads")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .count()
    }

    /// Sends one request and reads the answer.
    pub(crate) async fn send(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        headers: &[(header::HeaderName, &str)],
        body: Option<String>,
    ) -> Answer {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, token);
        }
        for (name, value) in headers {
            request = request.header(name, *value);
        }
        let response = self
            .app
            .clone()
            .oneshot(
                request
                    .body(body.map_or_else(Body::empty, Body::from))
                    .expect("a legal request"),
            )
            .await
            .expect("the router answers");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
            .await
            .expect("the body reads");
        Answer {
            status,
            headers,
            body: String::from_utf8(bytes.to_vec()).expect("UTF-8"),
        }
    }

    /// ITI-104: `PUT /fhir/Patient?identifier=system|value` with `body` in
    /// JSON.
    pub(crate) async fn feed(&self, system: &str, value: &str, body: &Document) -> Answer {
        self.send(
            Method::PUT,
            &format!("/fhir/Patient?{}", super::feeds::condition(system, value)),
            Some(&self.source()),
            &[(header::CONTENT_TYPE, "application/fhir+json")],
            Some(serde_json::to_string(body).expect("JSON")),
        )
        .await
    }

    /// ITI-83 by `GET` from `system|value`, asking for `targets`.
    pub(crate) async fn pix_get(&self, system: &str, value: &str, targets: &[&str]) -> Answer {
        self.send(
            Method::GET,
            &pix_uri(system, value, targets),
            Some(&self.consumer()),
            &[],
            None,
        )
        .await
    }

    /// ITI-83 by `POST` of `parameters` in JSON.
    pub(crate) async fn pix_post(&self, parameters: &str) -> Answer {
        self.send(
            Method::POST,
            "/fhir/Patient/$ihe-pix",
            Some(&self.consumer()),
            &[(header::CONTENT_TYPE, "application/fhir+json")],
            Some(parameters.to_owned()),
        )
        .await
    }
}

/// One answer.
pub(crate) struct Answer {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
}

impl Answer {
    /// The body as JSON.
    pub(crate) fn json(&self) -> Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|_| panic!("a JSON body: {} {}", self.status, self.body))
    }

    /// The media type.
    fn media_type(&self) -> &str {
        self.headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
    }

    /// The first issue's code and diagnostics.
    pub(crate) fn issue(&self) -> (String, String) {
        let outcome = self.json();
        let issue = &outcome["issue"][0];
        (
            issue["code"].as_str().unwrap_or_default().to_owned(),
            issue["diagnostics"].as_str().unwrap_or_default().to_owned(),
        )
    }

    /// The `targetIdentifier`s of a `Parameters` answer, as `system|value`.
    pub(crate) fn identifiers(&self) -> BTreeSet<String> {
        identifiers(&self.json())
    }

    /// The `targetId`s of a `Parameters` answer.
    fn target_ids(&self) -> Vec<String> {
        self.json()["parameter"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["name"] == "targetId")
            .filter_map(|p| p["valueReference"]["reference"].as_str().map(str::to_owned))
            .collect()
    }
}

/// The `targetIdentifier`s of `parameters`, as `system|value`.
fn identifiers(parameters: &Value) -> BTreeSet<String> {
    parameters["parameter"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["name"] == "targetIdentifier")
        .map(|p| {
            format!(
                "{}|{}",
                p["valueIdentifier"]["system"].as_str().unwrap_or_default(),
                p["valueIdentifier"]["value"].as_str().unwrap_or_default()
            )
        })
        .collect()
}

/// Percent-encodes `text` as a query value.
fn encoded(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

/// The ITI-83 `GET` URI from `system|value`, asking for `targets`.
pub(crate) fn pix_uri(system: &str, value: &str, targets: &[&str]) -> String {
    let mut uri = format!(
        "/fhir/Patient/$ihe-pix?sourceIdentifier={}",
        encoded(&format!("{system}|{value}"))
    );
    for target in targets {
        let _written = write!(uri, "&targetSystem={}", encoded(target));
    }
    uri
}

/// The `Parameters` of an ITI-83 `POST` from `system|value`, asking for
/// `targets`.
pub(crate) fn pix_parameters(system: &str, value: &str, targets: &[&str]) -> String {
    let mut parameter = vec![serde_json::json!({
        "name": "sourceIdentifier",
        "valueString": format!("{system}|{value}"),
    })];
    parameter.extend(
        targets
            .iter()
            .map(|target| serde_json::json!({"name": "targetSystem", "valueString": target})),
    );
    serde_json::json!({"resourceType": "Parameters", "parameter": parameter}).to_string()
}

/// Returns the fixture's document.
pub(crate) fn fixture(fixture: Fixture) -> Document {
    fixture.document().expect("the fixture reads")
}

/// Returns `document` as FHIR XML.
fn xml(document: &Document) -> String {
    fhir_types::xml::to_xml(&fhir_types::r4::schema::SCHEMAS, object(document))
        .expect("the document has an XML form")
}

/// Returns the object `document` is.
fn object(document: &Document) -> &Object {
    document.as_object().expect("a resource is an object")
}

/// Returns `document` with `system|value` appended to its identifiers.
fn carrying(document: &Document, system: &str, value: &str) -> Document {
    let mut json: Value = serde_json::to_value(document).expect("JSON");
    json["identifier"]
        .as_array_mut()
        .expect("identifiers")
        .push(serde_json::json!({"system": system, "value": value}));
    serde_json::from_value(json).expect("a document")
}

/// Returns `document` as the PIXm 3.1.0 vendored example `file`.
fn vendored(file: &str) -> Document {
    let path = format!(
        "{}/../../docs/specs/ihe-pixm/package/example/{file}",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut json: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the example reads"))
            .expect("JSON");
    // NOTE: FHIR R4 Narrative, the text is a rendering the Source need not send.
    json.as_object_mut().expect("an object").remove("text");
    serde_json::from_value(json).expect("a document")
}

/// Returns the hospital's value and the BSN the hospital fixture carries.
pub(crate) fn hospital_identifiers() -> (String, String) {
    let json: Value = serde_json::to_value(fixture(Fixture::FeedHospital)).expect("JSON");
    let value_in = |system: &str| {
        json["identifier"]
            .as_array()
            .expect("identifiers")
            .iter()
            .find(|i| i["system"] == system)
            .and_then(|i| i["value"].as_str())
            .expect("the identifier")
            .to_owned()
    };
    (value_in(HOSPITAL_DOMAIN), value_in(BSN_SYSTEM))
}

/// The laboratory's value in its fixture.
pub(crate) const LAB: &str = "L-200001";

/// Feeds the hospital and the laboratory, the laboratory carrying the BSN.
pub(crate) async fn feed_both(harness: &Harness) -> (String, String) {
    let (hospital, bsn) = hospital_identifiers();
    let fed = harness
        .feed(HOSPITAL_DOMAIN, &hospital, &fixture(Fixture::FeedHospital))
        .await;
    assert_eq!(StatusCode::CREATED, fed.status, "{}", fed.body);
    let laboratory = carrying(&fixture(Fixture::FeedLaboratory), BSN_SYSTEM, &bsn);
    let fed = harness.feed(LABORATORY_DOMAIN, LAB, &laboratory).await;
    assert_eq!(StatusCode::CREATED, fed.status, "{}", fed.body);
    (hospital, bsn)
}
