// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What the PDQm cases share: the store fed with synthetic patients through
//! the feed transaction, the server's routes over it behind the guard, a
//! request with a body and headers, the audit records the spool holds, and
//! the check of a document against a vendored PDQm profile.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the answers, the audit records and the vendored profiles are read as JSON"
)]

use std::path::{Path, PathBuf};

use axum::Router;
use axum::body::Body;
use ferropix_server::config::section::matching::Matching;
use ferropix_server::config::section::profiles::Deprecated;
use ferropix_server::feed::Feeder;
use ferropix_server::store::Store;
use ferropix_testkit::fixtures::Fixture;
use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES, HOSPITAL_DOMAIN, LABORATORY_DOMAIN};
use http::{Method, Request, StatusCode, header};
use ihe_iti::pixm::server::feed::Feed;
use ihe_iti::server::format::Format;
use serde_json::Value;
use tower::ServiceExt as _;
use uuid::Uuid;

use super::BSN_SYSTEM;
use super::feeds::{Started, condition, context, feeder, start};
use crate::auth::{Issuer, application_with, bearer, settings};
use crate::support::Secrets;

/// The FHIR base every served URL is written under: the authorization
/// cases' public base URL.
pub(crate) const BASE: &str = "http://localhost:8080/fhir";

/// A synthetic patient as a Source feeds it, with every element the search
/// reads.
#[derive(Debug, Clone)]
pub(crate) struct Fed {
    /// The domain the condition names.
    pub system: &'static str,
    /// The identifier value in it.
    pub value: String,
    /// The other identifiers the body carries.
    pub carried: Vec<(&'static str, String)>,
    /// The family name.
    pub family: String,
    /// The given names.
    pub given: Vec<String>,
    /// The administrative gender.
    pub gender: &'static str,
    /// The birth date.
    pub birth: String,
    /// The address line, city, state, postal code and country.
    pub address: Option<[String; 5]>,
    /// The telephone number.
    pub phone: Option<String>,
    /// The mother's maiden name.
    pub mother: Option<String>,
}

impl Fed {
    /// Returns a patient `value` in `system` named `family`, `given`.
    pub(crate) fn new(system: &'static str, value: &str, family: &str, given: &str) -> Self {
        Self {
            system,
            value: value.to_owned(),
            carried: Vec::new(),
            family: family.to_owned(),
            given: vec![given.to_owned()],
            gender: "female",
            birth: String::from("1961-04-12"),
            address: None,
            phone: None,
            mother: None,
        }
    }

    /// Returns the `Patient` body.
    pub(crate) fn body(&self) -> String {
        let identifiers: Vec<String> = std::iter::once((self.system, self.value.as_str()))
            .chain(self.carried.iter().map(|(s, v)| (*s, v.as_str())))
            .map(|(system, value)| format!(r#"{{"system":"{system}","value":"{value}"}}"#))
            .collect();
        let given: Vec<String> = self.given.iter().map(|g| format!(r#""{g}""#)).collect();
        let mut members = vec![
            format!(r#""identifier":[{}]"#, identifiers.join(",")),
            format!(
                r#""name":[{{"use":"official","family":"{}","given":[{}]}}]"#,
                self.family,
                given.join(",")
            ),
            format!(r#""gender":"{}""#, self.gender),
            format!(r#""birthDate":"{}""#, self.birth),
        ];
        if let Some([line, city, state, postal, country]) = &self.address {
            members.push(format!(
                r#""address":[{{"use":"home","line":["{line}"],"city":"{city}","state":"{state}","postalCode":"{postal}","country":"{country}"}}]"#
            ));
        }
        if let Some(phone) = &self.phone {
            members.push(format!(
                r#""telecom":[{{"system":"phone","value":"{phone}"}}]"#
            ));
        }
        if let Some(mother) = &self.mother {
            members.push(format!(
                r#""extension":[{{"url":"http://hl7.org/fhir/StructureDefinition/patient-mothersMaidenName","valueString":"{mother}"}}]"#
            ));
        }
        format!(r#"{{"resourceType":"Patient",{}}}"#, members.join(","))
    }
}

/// Feeds `body` for `value` in `system` and returns the person it is in.
pub(crate) async fn feed(feeder: &Feeder, system: &str, value: &str, body: &str) -> Uuid {
    let Ok(Feed::Revise(revision)) =
        Feed::from_update(&condition(system, value), Format::Json, body.as_bytes())
    else {
        panic!("an add or revise message");
    };
    feeder
        .revise(&revision, &context())
        .await
        .expect("the feed commits")
        .person
        .expect("a person")
        .uuid()
}

/// The persons of the shared synthetic population.
#[derive(Debug, Clone)]
pub(crate) struct Population {
    /// The hospital's record of Ada, with the BSN: the testkit fixture.
    pub ada_hospital: Uuid,
    /// The laboratory's record of Ada, a person of its own.
    pub ada_laboratory: Uuid,
    /// Bram, with every element the search reads.
    pub bram: Uuid,
    /// Cato, whose family name carries an accent.
    pub cato: Uuid,
    /// The duplicate the hospital merged into Ada's person.
    pub merged: Uuid,
}

/// The synthetic BSN Bram's record carries and no Source feeds.
pub(crate) fn carried_bsn() -> String {
    ferropix_testkit::synthetic::bsn(5)
        .expect("a synthetic BSN")
        .digits()
}

/// Bram's phone number, which no real subscriber holds.
pub(crate) const PHONE: &str = "+31600000001";

/// The accented spelling of the first synthetic family name.
pub(crate) const ACCENTED: &str = "Próefsma";

/// Feeds the shared population into `store`.
pub(crate) async fn populate(store: &Store) -> Population {
    let feeder = feeder(store);
    let hospital = Fixture::FeedHospital.text().expect("the fixture reads");
    let ada_hospital = feed(&feeder, HOSPITAL_DOMAIN, "H-100001", &hospital).await;
    let mut national = Fed::new(BSN_SYSTEM, FIXTURE_BSN, FAMILY_NAMES[0], GIVEN_NAMES[0]);
    national.address = Some([
        String::from("Voorbeeldstraat 1"),
        String::from("Teststad"),
        String::from("Proefprovincie"),
        String::from("0000 AA"),
        String::from("NL"),
    ]);
    let joined = feed(&feeder, BSN_SYSTEM, FIXTURE_BSN, &national.body()).await;
    assert_eq!(
        ada_hospital, joined,
        "the BSN joins the hospital record that carries it"
    );
    let laboratory = Fixture::FeedLaboratory.text().expect("the fixture reads");
    let ada_laboratory = feed(&feeder, LABORATORY_DOMAIN, "L-200001", &laboratory).await;
    let mut bram = Fed::new(HOSPITAL_DOMAIN, "H-300001", FAMILY_NAMES[1], GIVEN_NAMES[1]);
    bram.gender = "male";
    bram.birth = String::from("1970-01-01");
    bram.given.push(String::from("Daan"));
    bram.address = Some([
        String::from("Proeflaan 2"),
        String::from("Monsterdam"),
        String::from("Proefprovincie"),
        String::from("9999 ZZ"),
        String::from("NL"),
    ]);
    bram.phone = Some(PHONE.to_owned());
    bram.carried.push((BSN_SYSTEM, carried_bsn()));
    bram.mother = Some(FAMILY_NAMES[4].to_owned());
    let bram = feed(&feeder, HOSPITAL_DOMAIN, "H-300001", &bram.body()).await;
    let mut cato = Fed::new(HOSPITAL_DOMAIN, "H-400001", ACCENTED, GIVEN_NAMES[2]);
    cato.birth = String::from("1985-07");
    let cato = feed(&feeder, HOSPITAL_DOMAIN, "H-400001", &cato.body()).await;
    let duplicate = Fed::new(HOSPITAL_DOMAIN, "H-100099", FAMILY_NAMES[0], GIVEN_NAMES[0]);
    let merged = feed(&feeder, HOSPITAL_DOMAIN, "H-100099", &duplicate.body()).await;
    let resolved = Fixture::FeedDuplicateResolved
        .text()
        .expect("the fixture reads");
    let Ok(Feed::ResolveDuplicate(merge)) = Feed::from_update(
        &condition(HOSPITAL_DOMAIN, "H-100099"),
        Format::Json,
        resolved.as_bytes(),
    ) else {
        panic!("a resolve duplicate message");
    };
    let applied = feeder
        .resolve_duplicate(&merge, &context())
        .await
        .expect("the duplicate is resolved");
    assert_eq!(
        Some(ada_hospital),
        applied
            .person
            .map(ferropix_server::store::record::PersonId::uuid),
        "the survivor is Ada's person"
    );
    assert_ne!(
        merged, ada_hospital,
        "the duplicate was a person of its own"
    );
    Population {
        ada_hospital,
        ada_laboratory,
        bram,
        cato,
        merged,
    }
}

/// The served routes over a started store, and what a case reads back.
pub(crate) struct Served {
    /// The container and the store.
    pub started: Started,
    /// The routes, behind the guard.
    pub app: Router,
    /// The authorization server.
    pub issuer: Issuer,
    /// The audit spool's directory, removed on drop.
    pub spool: tempfile::TempDir,
    /// The secret files of the configuration, removed on drop.
    pub _secrets: Secrets,
}

impl Served {
    /// Returns a token of a Consumer that reads `Patient`.
    pub(crate) fn consumer(&self) -> String {
        bearer(&self.issuer.token("east-lab", "system/Patient.rs"))
    }

    /// Returns the records the audit spool holds.
    pub(crate) fn records(&self) -> Vec<Value> {
        records(&self.spool.path().join("spool"))
    }
}

/// Starts a store, and the routes over it answering a merged person by
/// `deprecated` with `matching` as the `[matching]` section.
pub(crate) async fn serve(deprecated: Deprecated, matching: &Matching) -> Served {
    let started = start().await;
    let app = over(&started.store, deprecated, matching).await;
    Served {
        started,
        app: app.0,
        issuer: app.1,
        spool: app.2,
        _secrets: app.3,
    }
}

/// Returns the routes over `store`, with their issuer, spool and secrets.
pub(crate) async fn over(
    store: &Store,
    deprecated: Deprecated,
    matching: &Matching,
) -> (Router, Issuer, tempfile::TempDir, Secrets) {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let settings = settings(&issuer, &secrets);
    let backend = ferropix_server::fhir::Backend {
        store: store.clone(),
        matching: matching.resolve().expect("the section resolves"),
        deprecated,
        configured: ferropix_server::operator::Configured::default(),
        development: false,
    };
    let (app, _, spool) = application_with(settings, &backend).await;
    (app, issuer, spool, secrets)
}

/// What one request answered.
#[derive(Debug)]
pub(crate) struct Answered {
    /// The status.
    pub status: StatusCode,
    /// The `Content-Type`.
    pub media_type: String,
    /// The body.
    pub body: String,
}

impl Answered {
    /// Returns the body as JSON.
    pub(crate) fn json(&self) -> Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|error| panic!("a JSON body ({error}): {}", self.body))
    }

    /// Returns the logical ids of the entries of `mode`, in order.
    pub(crate) fn ids(&self, mode: &str) -> Vec<Uuid> {
        self.json()["entry"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry["search"]["mode"] == mode)
            .filter_map(|entry| entry["resource"]["id"].as_str())
            .map(|id| Uuid::parse_str(id).expect("a person id"))
            .collect()
    }

    /// Returns the first issue's severity, code and diagnostics.
    pub(crate) fn issue(&self) -> (String, String, String) {
        let json = self.json();
        let issue = &json["issue"][0];
        let text = |key: &str| issue[key].as_str().unwrap_or_default().to_owned();
        (text("severity"), text("code"), text("diagnostics"))
    }
}

/// Sends `method` on `uri` with `headers` and `body`.
pub(crate) async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Answered {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body)).expect("a legal request"))
        .await
        .expect("the router answers");
    let status = response.status();
    let media_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = axum::body::to_bytes(response.into_body(), 64 << 20)
        .await
        .expect("the body reads");
    Answered {
        status,
        media_type,
        body: String::from_utf8(bytes.to_vec()).expect("a UTF-8 body"),
    }
}

/// Searches with `query` by `GET` and by `POST` of the same form, asserts
/// that both answer alike and returns the `GET` answer.
pub(crate) async fn search_both(app: &Router, token: &str, query: &str) -> Answered {
    let get = request(
        app,
        Method::GET,
        &format!("/fhir/Patient?{query}"),
        &[("authorization", token)],
        Vec::new(),
    )
    .await;
    let post = request(
        app,
        Method::POST,
        "/fhir/Patient/_search",
        &[
            ("authorization", token),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        query.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(get.status, post.status, "{query}: GET and POST");
    assert_eq!(get.media_type, post.media_type, "{query}: the format");
    if get.status == StatusCode::OK && get.media_type.starts_with("application/fhir+json") {
        assert_eq!(get.ids("match"), post.ids("match"), "{query}: the matches");
        assert_eq!(get.json()["total"], post.json()["total"], "{query}: total");
    }
    let xml = request(
        app,
        Method::GET,
        &format!("/fhir/Patient?{query}"),
        &[("authorization", token), ("accept", "application/fhir+xml")],
        Vec::new(),
    )
    .await;
    same_in_xml(&get, &xml, query);
    get
}

/// Holds the answer `xml`, asked for in XML, to the JSON answer `json` of
/// the same request: the same status and, when JSON was asked for and the
/// format was not the refusal, the same entries in FHIR XML.
pub(crate) fn same_in_xml(json: &Answered, xml: &Answered, label: &str) {
    assert_eq!(json.status, xml.status, "{label}: JSON and XML");
    if !json.media_type.starts_with("application/fhir+json")
        || json.status == StatusCode::NOT_ACCEPTABLE
    {
        return;
    }
    assert!(
        xml.media_type.starts_with("application/fhir+xml"),
        "{label}: {}",
        xml.media_type
    );
    if json.status == StatusCode::OK {
        let entries = json.json()["entry"].as_array().map_or(0, Vec::len);
        assert_eq!(
            entries,
            xml.body.matches("<entry>").count(),
            "{label}: the entries in XML"
        );
    } else {
        assert!(
            xml.body.contains("<OperationOutcome"),
            "{label}: {}",
            xml.body
        );
    }
}

/// Returns the records the spool at `directory` holds, by file name.
pub(crate) fn records(directory: &Path) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .filter_map(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(|record| record["resourceType"] == "AuditEvent")
        .collect()
}

/// Returns the `Patient/{id}` each record names as its patient, in order.
pub(crate) fn patients_of(records: &[Value]) -> Vec<Option<String>> {
    records
        .iter()
        .map(|record| {
            record["entity"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|entity| entity["type"]["code"] == "1" && entity["role"]["code"] == "1")
                .and_then(|entity| entity["what"]["reference"].as_str())
                .map(str::to_owned)
        })
        .collect()
}

/// Returns the raw request a record holds, base64-decoded.
pub(crate) fn query_of(record: &Value) -> String {
    use base64::Engine as _;
    let text = record["entity"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|entity| entity["query"].as_str())
        .expect("a query entity");
    String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .expect("base64"),
    )
    .expect("UTF-8")
}

/// The vendored PDQm profile `name`.
pub(crate) fn profile(name: &str) -> Value {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../docs/specs/ihe-pdqm/package",
        &format!("StructureDefinition-IHE.PDQm.{name}.json"),
    ]
    .iter()
    .collect();
    serde_json::from_str(&std::fs::read_to_string(&path).expect("the vendored profile"))
        .expect("JSON")
}

/// A vendored PDQm example, by its file name.
pub(crate) fn example(file: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../docs/specs/ihe-pdqm/package/example",
        file,
    ]
    .iter()
    .collect();
    std::fs::read_to_string(&path).expect("the vendored example")
}

/// Holds `document` to every cardinality, fixed value and pattern of the
/// differential of `profile`, slice by slice, and each `Patient` entry of a
/// Bundle to the PDQm Patient profile its slice names.
pub(crate) fn holds_to(document: &Value, profile: &Value) {
    let url = profile["url"].as_str().expect("a profile url");
    for element in profile["differential"]["element"]
        .as_array()
        .expect("a differential")
    {
        let id = element["id"].as_str().expect("an element id");
        let segments: Vec<&str> = id.split('.').collect();
        let Some((last, parents)) = segments.split_last() else {
            continue;
        };
        if parents.is_empty() {
            assert_eq!(document["resourceType"], *last, "{url}: the resource type");
            continue;
        }
        let mut nodes = vec![document.clone()];
        for segment in &parents[1..] {
            nodes = nodes
                .iter()
                .flat_map(|node| children(node, segment))
                .collect();
        }
        let min = element["min"].as_u64();
        let max = element["max"].as_str();
        for parent in &nodes {
            let found = children(parent, last);
            if let Some(min) = min {
                assert!(
                    u64::try_from(found.len()).unwrap() >= min,
                    "{url}: {id} holds {} of at least {min} in {parent}",
                    found.len()
                );
            }
            match max {
                Some("*") | None => {}
                Some(max) => {
                    let max: u64 = max.parse().expect("a maximum");
                    assert!(
                        u64::try_from(found.len()).unwrap() <= max,
                        "{url}: {id} holds {} of at most {max}",
                        found.len()
                    );
                }
            }
            for (key, value) in element.as_object().expect("an element") {
                if key.starts_with("fixed") || key.starts_with("pattern") {
                    for child in &found {
                        assert_eq!(child, value, "{url}: {id} {key}");
                    }
                }
            }
            if let Some(target) = element["type"][0]["profile"][0].as_str()
                && let Some(name) = target
                    .strip_prefix("https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.")
            {
                for child in &found {
                    holds_to(child, &self::profile(name));
                }
            }
        }
    }
}

/// The children of `node` a differential segment names: `name` or
/// `name:slice`, a choice `value[x]` read as any `value…` member.
fn children(node: &Value, segment: &str) -> Vec<Value> {
    let (name, slice) = match segment.split_once(':') {
        Some((name, slice)) => (name, Some(slice)),
        None => (segment, None),
    };
    let members: Vec<Value> = match name.strip_suffix("[x]") {
        Some(prefix) => node
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, _)| key.starts_with(prefix) && key.len() > prefix.len())
            .map(|(_, value)| value.clone())
            .collect(),
        None => match &node[name] {
            Value::Null => Vec::new(),
            Value::Array(items) => items.clone(),
            other => vec![other.clone()],
        },
    };
    let Some(slice) = slice else {
        return members;
    };
    members
        .into_iter()
        .filter(|member| in_slice(name, slice, member))
        .collect()
}

/// Returns whether `member` of `name` belongs to the slice `slice`: an
/// entry by its resource type, an extension by its URL, a parameter by its
/// name.
fn in_slice(name: &str, slice: &str, member: &Value) -> bool {
    match name {
        "entry" => member["resource"]["resourceType"]
            .as_str()
            .is_some_and(|kind| kind.eq_ignore_ascii_case(slice)),
        "extension" => member["url"].as_str().is_some_and(|url| {
            let tail = url.rsplit('/').next().unwrap_or_default();
            tail.eq_ignore_ascii_case(slice)
                || tail.replace('-', "").eq_ignore_ascii_case(slice)
                || tail
                    .strip_prefix("patient-")
                    .or_else(|| tail.strip_prefix("individual-"))
                    .is_some_and(|rest| rest.eq_ignore_ascii_case(slice))
        }),
        "parameter" => member["name"] == slice,
        _ => true,
    }
}

/// The synthetic BSN the hospital fixture carries.
pub(crate) const FIXTURE_BSN: &str = "999990007";

/// The BSN system, for a query.
pub(crate) fn bsn_token(value: &str) -> String {
    format!(
        "{}%7C{value}",
        BSN_SYSTEM.replace(':', "%3A").replace('/', "%2F")
    )
}
