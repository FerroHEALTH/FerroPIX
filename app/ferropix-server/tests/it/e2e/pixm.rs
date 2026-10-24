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

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use ferropix_server::audit::Audit;
use ferropix_server::auth::Smart;
use ferropix_server::config::section::domain::{DomainKind, DomainSeed};
use ferropix_server::fhir::{self, Backend, Mounts};
use ferropix_server::health::Registry;
use ferropix_server::http::router;
use ferropix_server::store::{Job, Store};
use ferropix_testkit::containers::{Postgres, e2e_enabled, postgres};
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
    _postgres: Postgres,
    _secrets: Secrets,
}

impl Harness {
    /// Starts PostgreSQL, the store with `seeds`, and the router.
    pub(crate) async fn start(seeds: Vec<DomainSeed>) -> Self {
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
        let smart = Arc::new(
            Smart::discover(settings.auth, &settings.domains)
                .await
                .expect("the authorization server is read"),
        );
        let spool = tempfile::tempdir().expect("a temporary directory");
        let mut audit_settings = settings.audit.clone();
        audit_settings.spool_dir = spool.path().join("spool");
        let audit = Audit::open(&audit_settings, &settings.server.public_base_url)
            .expect("the audit spool opens");
        let health = Registry::default();
        let routes = fhir::routes(
            &settings.server.base_path,
            &settings.server.public_base_url,
            Mounts::all(),
            &smart,
            &health,
            &audit,
            &Backend {
                store: store.clone(),
                matching: settings.matching.clone(),
                deprecated: settings.pdqm.deprecated,
            },
        );
        Self {
            app: router(&settings.server, &health, Some(routes)),
            issuer,
            spool,
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

    /// The Source's token.
    pub(crate) fn source(&self) -> String {
        bearer(&self.issuer.token(SOURCE, "system/Patient.cud"))
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

#[tokio::test]
async fn a_fed_patient_is_answered_alike_by_get_and_post_in_json_and_xml() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let (hospital, bsn) = hospital_identifiers();
    let created = harness
        .feed(HOSPITAL_DOMAIN, &hospital, &fixture(Fixture::FeedHospital))
        .await;
    assert_eq!(StatusCode::CREATED, created.status, "{}", created.body);
    assert_eq!(
        (
            "informational".to_owned(),
            "the identifier is added to the cross-reference".to_owned()
        ),
        created.issue()
    );
    let location = created
        .headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a Location");
    assert!(
        location.starts_with("http://localhost:8080/fhir/Patient/"),
        "{location}"
    );
    let revised = harness
        .feed(HOSPITAL_DOMAIN, &hospital, &fixture(Fixture::FeedHospital))
        .await;
    assert_eq!(StatusCode::OK, revised.status, "FHIR R4 http.html §3.1.0.4");
    let laboratory = carrying(&fixture(Fixture::FeedLaboratory), BSN_SYSTEM, &bsn);
    let in_xml = harness
        .send(
            Method::PUT,
            &format!(
                "/fhir/Patient?{}&_format=xml",
                super::feeds::condition(LABORATORY_DOMAIN, LAB)
            ),
            Some(&harness.source()),
            &[(header::CONTENT_TYPE, "application/fhir+xml")],
            Some(xml(&laboratory)),
        )
        .await;
    assert_eq!(StatusCode::CREATED, in_xml.status, "{}", in_xml.body);
    assert_eq!("application/fhir+xml", in_xml.media_type());
    assert!(in_xml.body.contains("<OperationOutcome"), "{}", in_xml.body);

    let expected = identifiers(&serde_json::to_value(fixture(Fixture::PixQueryOut)).expect("JSON"));
    let got = harness
        .pix_get(LABORATORY_DOMAIN, LAB, &[HOSPITAL_DOMAIN, BSN_SYSTEM])
        .await;
    assert_eq!(StatusCode::OK, got.status, "{}", got.body);
    assert_eq!(expected, got.identifiers(), "PIXm 3.1.0 §2:3.83.4.2.2.1");
    let ids = got.target_ids();
    assert_eq!(1, ids.len(), "the person is the one Patient");
    assert_eq!(
        Some(location.trim_start_matches("http://localhost:8080/fhir/")),
        ids.first().map(String::as_str)
    );
    let posted = harness
        .pix_post(&Fixture::PixQueryIn.text().expect("the fixture reads"))
        .await;
    assert_eq!(StatusCode::OK, posted.status, "{}", posted.body);
    assert_eq!(
        got.body, posted.body,
        "POST answers exactly what GET answers (#52)"
    );

    let posted_xml = harness
        .send(
            Method::POST,
            "/fhir/Patient/$ihe-pix",
            Some(&harness.consumer()),
            &[
                (header::CONTENT_TYPE, "application/fhir+xml"),
                (header::ACCEPT, "application/fhir+xml"),
            ],
            Some(xml(&fixture(Fixture::PixQueryIn))),
        )
        .await;
    assert_eq!(StatusCode::OK, posted_xml.status, "{}", posted_xml.body);
    assert_eq!("application/fhir+xml", posted_xml.media_type());
    let decoded = fhir_types::xml::from_xml(&fhir_types::r4::schema::SCHEMAS, &posted_xml.body)
        .expect("the answer is FHIR XML");
    let decoded: Value = serde_json::to_value(Document::Object(decoded)).expect("JSON");
    assert_eq!(got.json(), decoded, "the XML answer is the JSON answer");

    let every = harness.pix_get(LABORATORY_DOMAIN, LAB, &[]).await;
    assert_eq!(
        BTreeSet::from([
            format!("{HOSPITAL_DOMAIN}|{hospital}"),
            format!("{BSN_SYSTEM}|{bsn}")
        ]),
        every.identifiers(),
        "every identifier but the one asked (§2:3.83.4.2.2.1)"
    );
    assert_eq!(7, harness.records(), "one BALP record per transaction");
}

#[tokio::test]
async fn every_refusal_of_iti_83_is_the_one_the_table_names() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let (hospital, _bsn) = feed_both(&harness).await;
    let before = harness.records();
    for (label, uri_targets, source, status, code, diagnostics) in [
        (
            "an unknown source domain",
            &[][..],
            ("urn:oid:2.999.9.9", "X-1"),
            StatusCode::BAD_REQUEST,
            "code-invalid",
            "sourceIdentifier Assigning Authority not found",
        ),
        (
            "an unknown target domain",
            &["urn:oid:2.999.9.9"][..],
            (HOSPITAL_DOMAIN, hospital.as_str()),
            StatusCode::FORBIDDEN,
            "code-invalid",
            "targetSystem not found",
        ),
        (
            "an unknown identifier",
            &[][..],
            (HOSPITAL_DOMAIN, "H-100777"),
            StatusCode::NOT_FOUND,
            "not-found",
            "sourceIdentifier Patient Identifier not found",
        ),
    ] {
        let got = harness.pix_get(source.0, source.1, uri_targets).await;
        assert_eq!(status, got.status, "GET: {label}");
        assert_eq!(
            (code.to_owned(), diagnostics.to_owned()),
            got.issue(),
            "GET: {label}"
        );
        let posted = harness
            .pix_post(&pix_parameters(source.0, source.1, uri_targets))
            .await;
        assert_eq!(status, posted.status, "POST: {label}");
        assert_eq!(got.body, posted.body, "POST: {label}");
    }
    assert_eq!(
        before + 6,
        harness.records(),
        "each refused query is recorded"
    );
    let in_xml = harness
        .send(
            Method::GET,
            &format!("{}&_format=xml", pix_uri(HOSPITAL_DOMAIN, "H-100777", &[])),
            Some(&harness.consumer()),
            &[],
            None,
        )
        .await;
    assert_eq!(StatusCode::NOT_FOUND, in_xml.status);
    assert!(
        in_xml.body.contains("<code value=\"not-found\"/>"),
        "{}",
        in_xml.body
    );
    let missing = harness
        .send(
            Method::GET,
            "/fhir/Patient/$ihe-pix",
            Some(&harness.consumer()),
            &[],
            None,
        )
        .await;
    assert_eq!(StatusCode::BAD_REQUEST, missing.status);
    assert_eq!("required", missing.issue().0, "§2:3.83.4.1.2.1");
    let unacceptable = harness
        .send(
            Method::GET,
            &format!(
                "{}&_format=text/turtle",
                pix_uri(HOSPITAL_DOMAIN, &hospital, &[])
            ),
            Some(&harness.consumer()),
            &[],
            None,
        )
        .await;
    assert_eq!(StatusCode::NOT_ACCEPTABLE, unacceptable.status);
    assert_eq!("not-supported", unacceptable.issue().0);
    let plain = harness
        .send(
            Method::POST,
            "/fhir/Patient/$ihe-pix",
            Some(&harness.consumer()),
            &[(header::CONTENT_TYPE, "text/plain")],
            Some(String::from("sourceIdentifier")),
        )
        .await;
    assert_eq!(StatusCode::UNSUPPORTED_MEDIA_TYPE, plain.status);
}

#[tokio::test]
async fn every_refusal_of_iti_104_is_the_one_the_table_names() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let (hospital, _bsn) = feed_both(&harness).await;
    let stranger = bearer(&harness.issuer.token(STRANGER, "system/Patient.cud"));
    let foreign = harness
        .send(
            Method::PUT,
            &format!(
                "/fhir/Patient?{}",
                super::feeds::condition(HOSPITAL_DOMAIN, &hospital)
            ),
            Some(&stranger),
            &[(header::CONTENT_TYPE, "application/fhir+json")],
            Some(serde_json::to_string(&fixture(Fixture::FeedHospital)).expect("JSON")),
        )
        .await;
    assert_eq!(StatusCode::FORBIDDEN, foreign.status, "§2:3.104.5");
    assert_eq!("forbidden", foreign.issue().0);
    let uncarried = harness
        .feed(HOSPITAL_DOMAIN, "H-100555", &fixture(Fixture::FeedHospital))
        .await;
    assert_eq!(StatusCode::BAD_REQUEST, uncarried.status, "§2:3.104.4.1.2");
    assert_eq!("invalid", uncarried.issue().0);
    let unknown = harness
        .feed(
            "urn:oid:2.999.9.9",
            "X-1",
            &carrying(&fixture(Fixture::FeedHospital), "urn:oid:2.999.9.9", "X-1"),
        )
        .await;
    assert_eq!(StatusCode::BAD_REQUEST, unknown.status);
    assert_eq!(
        (
            "code-invalid".to_owned(),
            "identifier Assigning Authority not found".to_owned()
        ),
        unknown.issue()
    );
    let broken_bsn = "999990001";
    let refused = harness
        .feed(
            BSN_SYSTEM,
            broken_bsn,
            &carrying(&fixture(Fixture::FeedLaboratory), BSN_SYSTEM, broken_bsn),
        )
        .await;
    assert_eq!(
        StatusCode::UNPROCESSABLE_ENTITY,
        refused.status,
        "{}",
        refused.body
    );
    assert_eq!("business-rule", refused.issue().0);
}

#[tokio::test]
async fn a_resolved_duplicate_answers_the_survivors_set_and_a_removed_identifier_is_not_found() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let (hospital, bsn) = feed_both(&harness).await;
    let duplicate_value = "H-100099";
    let mut duplicate: Value =
        serde_json::to_value(fixture(Fixture::FeedDuplicateResolved)).expect("JSON");
    let resolved: Document = serde_json::from_value(duplicate.clone()).expect("a document");
    duplicate.as_object_mut().expect("an object").remove("link");
    duplicate["active"] = Value::Bool(true);
    let duplicate: Document = serde_json::from_value(duplicate).expect("a document");
    let fed = harness
        .feed(HOSPITAL_DOMAIN, duplicate_value, &duplicate)
        .await;
    assert_eq!(StatusCode::CREATED, fed.status, "{}", fed.body);
    let merged = harness
        .feed(HOSPITAL_DOMAIN, duplicate_value, &resolved)
        .await;
    assert_eq!(
        StatusCode::OK,
        merged.status,
        "§2:3.104.4.2.4: {}",
        merged.body
    );
    let subsumed = harness.pix_get(HOSPITAL_DOMAIN, duplicate_value, &[]).await;
    assert_eq!(StatusCode::OK, subsumed.status, "{}", subsumed.body);
    assert_eq!(
        BTreeSet::from([
            format!("{HOSPITAL_DOMAIN}|{hospital}"),
            format!("{BSN_SYSTEM}|{bsn}"),
            format!("{LABORATORY_DOMAIN}|{LAB}"),
        ]),
        subsumed.identifiers(),
        "§2:3.104.4.2.3: the survivor's references"
    );
    let survivor = harness.pix_get(HOSPITAL_DOMAIN, &hospital, &[]).await;
    assert!(
        !survivor
            .identifiers()
            .contains(&format!("{HOSPITAL_DOMAIN}|{duplicate_value}")),
        "the subsumed identifier is replaced"
    );
    let revised = harness
        .feed(HOSPITAL_DOMAIN, duplicate_value, &duplicate)
        .await;
    assert_eq!(StatusCode::UNPROCESSABLE_ENTITY, revised.status);

    let removal = format!(
        "/fhir/Patient?{}",
        super::feeds::condition(LABORATORY_DOMAIN, LAB)
    );
    let removed = harness
        .send(Method::DELETE, &removal, Some(&harness.source()), &[], None)
        .await;
    assert_eq!(StatusCode::OK, removed.status, "{}", removed.body);
    assert_eq!(
        "the identifier is removed from the cross-reference",
        removed.issue().1
    );
    let gone = harness.pix_get(LABORATORY_DOMAIN, LAB, &[]).await;
    assert_eq!(StatusCode::NOT_FOUND, gone.status, "§2:3.104.4.3.3");
    let survivor = harness.pix_get(HOSPITAL_DOMAIN, &hospital, &[]).await;
    assert!(
        !survivor
            .identifiers()
            .contains(&format!("{LABORATORY_DOMAIN}|{LAB}")),
        "the removed identifier is answered nowhere"
    );
    let again = harness
        .send(Method::DELETE, &removal, Some(&harness.source()), &[], None)
        .await;
    assert_eq!(StatusCode::OK, again.status, "FHIR R4 http.html §3.1.0.7");
    assert_eq!(
        "the cross-reference holds no such identifier",
        again.issue().1
    );
}

#[tokio::test]
async fn the_vendored_mohr_alice_set_round_trips() {
    if !e2e_enabled() {
        return;
    }
    // NOTE: our own design for the test; Red is authoritative so the Blue and
    // Green feeds that carry IHERED-994 join its person deterministically.
    let harness = Harness::start(vec![
        seed(RED, DomainKind::Authoritative, false),
        seed(GREEN, DomainKind::Local, false),
        seed(BLUE, DomainKind::Local, false),
    ])
    .await;
    let red = harness
        .feed(
            RED,
            "IHERED-994",
            &vendored("Patient-Patient-MohrAlice-Red.json"),
        )
        .await;
    assert_eq!(StatusCode::CREATED, red.status, "{}", red.body);
    let alice = vendored("Patient-Patient-MohrAlice.json");
    for (system, value) in [(BLUE, "IHEBLUE-994"), (GREEN, "IHEGREEN-994")] {
        let fed = harness.feed(system, value, &alice).await;
        assert_eq!(StatusCode::CREATED, fed.status, "{system}: {}", fed.body);
    }
    let request =
        serde_json::to_string(&vendored("Parameters-pixm-request-mohralice-red-all.json"))
            .expect("JSON");
    let all = harness.pix_post(&request).await;
    assert_eq!(StatusCode::OK, all.status, "{}", all.body);
    let response: Value =
        serde_json::to_value(vendored("Parameters-pixm-response-mohralice-red-all.json"))
            .expect("JSON");
    assert_eq!(identifiers(&response), all.identifiers());
    assert_eq!(1, all.target_ids().len(), "one Patient, the person");
    let to_blue = serde_json::to_string(&vendored(
        "Parameters-pixm-request-mohralice-red-to-blue.json",
    ))
    .expect("JSON");
    let in_xml = harness.pix_post(&to_blue).await;
    assert_eq!(StatusCode::OK, in_xml.status, "{}", in_xml.body);
    assert_eq!(
        "application/fhir+xml",
        in_xml.media_type(),
        "the posted _format parameter"
    );
    assert!(in_xml.body.contains("IHEBLUE-994"));

    let maiden = harness
        .feed(
            RED,
            "IHERED-m94",
            &vendored("Patient-Patient-MaidenAlice-Red.json"),
        )
        .await;
    assert_eq!(StatusCode::CREATED, maiden.status, "{}", maiden.body);
    let resolved = harness
        .feed(
            RED,
            "IHERED-m94",
            &vendored("Patient-Patient-MohrMaidenResolvedByMohrMalice-Red.json"),
        )
        .await;
    assert_eq!(StatusCode::OK, resolved.status, "{}", resolved.body);
    let subsumed = harness.pix_get(RED, "IHERED-m94", &[]).await;
    assert_eq!(StatusCode::OK, subsumed.status, "{}", subsumed.body);
    assert_eq!(
        BTreeSet::from([
            format!("{RED}|IHERED-994"),
            format!("{GREEN}|IHEGREEN-994"),
            format!("{BLUE}|IHEBLUE-994"),
        ]),
        subsumed.identifiers()
    );
}

#[tokio::test]
async fn a_carried_bsn_finds_its_person_and_answers_among_its_identifiers() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    let (hospital, bsn) = feed_both(&harness).await;
    let by_bsn = harness.pix_get(BSN_SYSTEM, &bsn, &[]).await;
    assert_eq!(StatusCode::OK, by_bsn.status, "{}", by_bsn.body);
    assert_eq!(
        BTreeSet::from([
            format!("{HOSPITAL_DOMAIN}|{hospital}"),
            format!("{LABORATORY_DOMAIN}|{LAB}"),
        ]),
        by_bsn.identifiers(),
        "the BSN itself is left out of its own answer (§2:3.83.4.2.2.1)"
    );
    let posted = harness
        .pix_post(&pix_parameters(BSN_SYSTEM, &bsn, &[HOSPITAL_DOMAIN]))
        .await;
    assert_eq!(StatusCode::OK, posted.status, "{}", posted.body);
    assert_eq!(
        BTreeSet::from([format!("{HOSPITAL_DOMAIN}|{hospital}")]),
        posted.identifiers()
    );

    let mut elsewhere: Value = serde_json::to_value(carrying(
        &fixture(Fixture::FeedLaboratory),
        BSN_SYSTEM,
        &bsn,
    ))
    .expect("JSON");
    elsewhere["identifier"][0] =
        serde_json::json!({"system": PRACTICE_DOMAIN, "value": "P-300001"});
    elsewhere["birthDate"] = Value::String(String::from("1980-02-02"));
    let elsewhere: Document = serde_json::from_value(elsewhere).expect("a document");
    let fed = harness.feed(PRACTICE_DOMAIN, "P-300001", &elsewhere).await;
    assert_eq!(StatusCode::CREATED, fed.status, "{}", fed.body);
    let practice = harness.pix_get(PRACTICE_DOMAIN, "P-300001", &[]).await;
    assert_eq!(StatusCode::OK, practice.status, "{}", practice.body);
    assert!(
        practice
            .identifiers()
            .contains(&format!("{BSN_SYSTEM}|{bsn}")),
        "the practice's own person carries the BSN too"
    );
    let ambiguous = harness.pix_get(BSN_SYSTEM, &bsn, &[]).await;
    assert_eq!(StatusCode::CONFLICT, ambiguous.status, "{}", ambiguous.body);
    assert_eq!("multiple-matches", ambiguous.issue().0);
}
