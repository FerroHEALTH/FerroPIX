// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The BALP audit trail: every record held to its vendored profile, the BALP
//! patterns beneath it and the profile's own example; the spool that refuses
//! the start or the request when it cannot take a record; the drain that
//! removes a delivered record and quarantines a refused one; and the audit
//! record as the only place an identifier reaches the disk or the log.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the records and the vendored profiles are read as JSON values"
)]

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ferrohealth_health::registry::Registry;
use ferrohealth_server::health::Health;
use ferrohealth_smart::resource::{Access, Caller};
use ferrohealth_telemetry::request_log::RequestLog;
use ferropix_server::audit::records::operator::{StewardAction, steward};
use ferropix_server::audit::records::{
    Call, Feed, Identifier, Search, feed, pdq_search, pix_query, raw_request,
};
use ferropix_server::audit::reserve::UNAUDITED;
use ferropix_server::audit::{Audit, Headroom, OpenError};
use ferropix_server::auth::Smart;
use ferropix_server::config::section::audit::AuditSettings;
use ferropix_server::fhir::{self, Mounts};
use ferropix_server::http::router;
use http::{Method, StatusCode};
use ihe_atna::atna::spool::{QUARANTINE, SpoolError};
use ihe_atna::balp::{Exchange, NetworkAddress, Observer, Outcome, SOURCE_APPLICATION_SERVER};
use secrecy::{ExposeSecret as _, SecretString};
use serde_json::Value;
use wiremock::matchers;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::auth::{Captured, EVERY_SCOPE, Issuer, bearer, send, settings as auth_settings};
use crate::support::Secrets;

/// The canonical URL prefix of the BALP patterns.
const BALP: &str = "https://profiles.ihe.net/ITI/BALP/StructureDefinition/";

/// The FHIR base the records name as the endpoint.
const BASE: &str = "https://pix.example.org/fhir";

/// The domain of the synthetic identifiers.
const DOMAIN: &str = "urn:oid:2.999.1.1";

/// The synthetic BSN system.
const BSN_SYSTEM: &str = "http://fhir.nl/fhir/NamingSystem/bsn";

/// A synthetic BSN from the reserved range.
fn bsn() -> String {
    ferropix_testkit::synthetic::bsn(1)
        .expect("the synthetic range holds a BSN")
        .digits()
}

/// The observer the tests record as.
fn observer() -> Observer {
    Observer {
        source_id: String::from("pix.example.org"),
        site: None,
        host: NetworkAddress::host("pix.example.org"),
        source_types: vec![SOURCE_APPLICATION_SERVER],
    }
}

/// The call of a backend client acting for itself, or of a user when
/// `user` names their subject.
fn call(user: Option<&str>) -> Call {
    let subject = user.unwrap_or("east-lab");
    let access = Access::Caller(Arc::new(
        Caller::new(
            subject.to_owned(),
            Some(String::from("east-lab")),
            "system/Patient.rs",
            None,
            false,
        )
        .with_issuer(String::from("https://idp.example.org")),
    ));
    Call::of(
        &access,
        Some("192.0.2.7".parse().expect("an address")),
        &url::Url::parse(BASE).expect("a URL"),
        Some("req-17"),
    )
}

/// The `AuditEvent` `exchange` is written as, decoded as R4 and read back as
/// JSON.
fn written(exchange: &Exchange) -> Value {
    let record = exchange
        .audit_event(&observer())
        .expect("a record")
        .into_bytes();
    let bytes = record.expose_secret();
    serde_json::from_slice::<fhir_types::r4::audit_event::AuditEvent>(bytes)
        .expect("the record reads back as an R4 AuditEvent");
    serde_json::from_slice(bytes).expect("JSON")
}

/// A vendored file, by its corpus directory and its path in it.
fn vendored(corpus: &str, file: &str) -> Value {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "../../docs/specs", corpus, file]
        .iter()
        .collect();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the vendored {} ({error})", path.display()));
    serde_json::from_str(&text).expect("vendored JSON")
}

/// The vendored BALP pattern `url` names.
fn balp(url: &str) -> Option<Value> {
    let name = url.strip_prefix(BALP)?;
    Some(vendored(
        "ihe-balp",
        &format!("package/StructureDefinition-{name}.json"),
    ))
}

/// `profile` and every BALP pattern beneath it.
fn chain(profile: Value) -> Vec<Value> {
    let mut chain = vec![profile];
    while let Some(base) = chain
        .last()
        .and_then(|last| last["baseDefinition"].as_str())
        .and_then(balp)
    {
        chain.push(base);
    }
    chain
}

/// The `(system, code)` of a coding.
fn code(coding: &Value) -> (String, String) {
    (
        coding["system"].as_str().unwrap_or_default().to_owned(),
        coding["code"].as_str().unwrap_or_default().to_owned(),
    )
}

/// The codings of every `agent.type`.
fn agent_codes(record: &Value) -> BTreeSet<(String, String)> {
    record["agent"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|agent| agent["type"]["coding"].as_array().into_iter().flatten())
        .map(code)
        .collect()
}

/// The `(type, role)` codes of each entity.
fn entity_codes(record: &Value) -> BTreeSet<(String, String)> {
    record["entity"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entity| (code(&entity["type"]).1, code(&entity["role"]).1))
        .collect()
}

/// Holds `record` to every fixed value and pattern the differentials of
/// `profile` and the BALP patterns beneath it fix: `meta.profile`, `type`,
/// each `subtype` slice, `action`, each required agent slice's type, and
/// each required entity slice's type and role.
fn holds_to(record: &Value, profile: &Value) {
    let url = profile["url"].as_str().expect("a profile url");
    assert_eq!(
        record["meta"]["profile"][0].as_str(),
        Some(url),
        "the record names its profile"
    );
    let subtypes: BTreeSet<(String, String)> = record["subtype"]
        .as_array()
        .into_iter()
        .flatten()
        .map(code)
        .collect();
    let agents = agent_codes(record);
    let entities = record["entity"].as_array().cloned().unwrap_or_default();
    for definition in chain(profile.clone()) {
        let elements = definition["differential"]["element"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let min_of = |slice: &str| {
            elements
                .iter()
                .find(|element| element["id"].as_str() == Some(slice))
                .and_then(|element| element["min"].as_u64())
        };
        for element in &elements {
            let id = element["id"].as_str().unwrap_or_default();
            let pattern = element
                .get("patternCoding")
                .or_else(|| element.get("fixedCoding"));
            let fixed = element
                .get("fixedCode")
                .or_else(|| element.get("patternCode"))
                .and_then(Value::as_str);
            match (id, pattern, fixed) {
                ("AuditEvent.type", Some(pattern), _) => {
                    assert_eq!(code(&record["type"]), code(pattern), "{url}: type");
                }
                ("AuditEvent.action", _, Some(action)) => {
                    assert_eq!(record["action"].as_str(), Some(action), "{url}: action");
                }
                (id, Some(pattern), _)
                    if id.starts_with("AuditEvent.subtype:") && !id.contains('.') =>
                {
                    assert!(subtypes.contains(&code(pattern)), "{url}: subtype {id}");
                }
                _ => {}
            }
            if let (Some(slice), Some(concept)) = (
                id.strip_suffix(".type")
                    .filter(|slice| slice.starts_with("AuditEvent.agent:")),
                element.get("patternCodeableConcept"),
            ) && min_of(slice).unwrap_or(0) > 0
            {
                for coding in concept["coding"].as_array().into_iter().flatten() {
                    assert!(
                        agents.contains(&code(coding)),
                        "{url}: {slice} type {coding}"
                    );
                }
            }
            if let (Some(slice), Some(pattern)) = (
                id.strip_suffix(".type")
                    .filter(|slice| slice.starts_with("AuditEvent.entity:")),
                element.get("patternCoding"),
            ) && min_of(slice).unwrap_or(0) > 0
            {
                let role = elements
                    .iter()
                    .find(|element| element["id"].as_str() == Some(&format!("{slice}.role")))
                    .and_then(|element| element.get("patternCoding"))
                    .map(code);
                assert!(
                    entities.iter().any(|entity| {
                        code(&entity["type"]) == code(pattern)
                            && role
                                .as_ref()
                                .is_none_or(|role| code(&entity["role"]) == *role)
                    }),
                    "{url}: {slice} as {pattern} with role {role:?}"
                );
            }
        }
    }
}

/// Holds `record` to the profile's own example: the same type, subtypes,
/// action, system agent types and entity types and roles, the transaction
/// entity of the request id left out.
fn like(record: &Value, example: &Value) {
    assert_eq!(code(&record["type"]), code(&example["type"]), "type");
    let subtypes = |event: &Value| -> BTreeSet<(String, String)> {
        event["subtype"]
            .as_array()
            .into_iter()
            .flatten()
            .map(code)
            .collect()
    };
    assert_eq!(subtypes(record), subtypes(example), "subtypes");
    assert_eq!(record["action"], example["action"], "action");
    assert_eq!(agent_codes(record), agent_codes(example), "agent types");
    let mut entities = entity_codes(record);
    entities.remove(&(String::from("XrequestId"), String::new()));
    assert_eq!(entities, entity_codes(example), "entities");
}

/// `text` base64-decoded, as UTF-8.
fn decoded(text: &str) -> String {
    use base64::Engine as _;
    String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .expect("base64"),
    )
    .expect("UTF-8")
}

/// The entity of `record` with the `(type, role)` codes.
fn entity<'a>(record: &'a Value, kind: &str, role: &str) -> Option<&'a Value> {
    record["entity"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entity| code(&entity["type"]).1 == kind && code(&entity["role"]).1 == role)
}

#[test]
fn every_record_holds_to_its_profile_and_names_the_patient_and_the_query() {
    let bsn = bsn();
    let source = Identifier {
        system: String::from(BSN_SYSTEM),
        value: SecretString::from(bsn.clone()),
    };
    let get = url::Url::parse(&format!(
        "{BASE}/Patient/$ihe-pix?sourceIdentifier={BSN_SYSTEM}%7C{bsn}"
    ))
    .expect("a URL");
    let body = SecretString::from(format!(
        r#"{{"resourceType":"Parameters","parameter":[{{"name":"sourceIdentifier","valueIdentifier":{{"system":"{BSN_SYSTEM}","value":"{bsn}"}}}}]}}"#
    ));
    let post = url::Url::parse(&format!("{BASE}/Patient/$ihe-pix")).expect("a URL");
    let pixm_query = vendored(
        "ihe-pixm",
        "package/StructureDefinition-IHE.PIXm.Query.Audit.Manager.json",
    );
    let pixm_example = vendored(
        "ihe-pixm",
        "package/example/AuditEvent-ex-auditPixmQuery-manager.json",
    );
    for (raw, sent) in [
        (raw_request(&get, None), get.to_string()),
        (
            raw_request(&post, Some(("application/fhir+json", &body))),
            String::from("POST"),
        ),
    ] {
        let record = written(&pix_query(&call(None), raw, &source, Outcome::Success));
        holds_to(&record, &pixm_query);
        like(&record, &pixm_example);
        let patient = entity(&record, "1", "1").expect("the patient entity");
        assert_eq!(BSN_SYSTEM, patient["what"]["identifier"]["system"]);
        assert_eq!(bsn.as_str(), patient["what"]["identifier"]["value"]);
        let query = entity(&record, "2", "24").expect("the query entity");
        let text = decoded(query["query"].as_str().expect("a base64 query"));
        assert!(
            text.contains(&sent),
            "BALP 1.1.4 §1:52.4.1.4: the raw request"
        );
        assert!(text.contains(&bsn), "the raw request as sent");
    }
    let fed = Identifier {
        system: String::from(DOMAIN),
        value: SecretString::from("MRN-SYNTHETIC-1"),
    };
    for (action, profile, example) in [
        (Feed::Create, "Create", "create"),
        (Feed::Update, "Update", "update"),
        (Feed::Delete, "Delete", "delete"),
    ] {
        let record = written(&feed(
            &call(None),
            action,
            &fed,
            Some("Patient/0192aeb0-0000-7000-8000-000000000001"),
            Outcome::Success,
        ));
        holds_to(
            &record,
            &vendored(
                "ihe-pixm",
                &format!("package/StructureDefinition-IHE.PIXm.Feed.{profile}.Audit.Manager.json"),
            ),
        );
        like(
            &record,
            &vendored(
                "ihe-pixm",
                &format!("package/example/AuditEvent-ex-auditPixmFeed-{example}-manager.json"),
            ),
        );
        let patient = entity(&record, "1", "1").expect("the patient entity");
        assert!(
            patient["what"]["reference"].is_null(),
            "{profile}: what.reference is 0..0"
        );
        assert_eq!("MRN-SYNTHETIC-1", patient["what"]["identifier"]["value"]);
    }
}

#[test]
fn every_supplier_and_steward_record_holds_to_its_profile() {
    let persons = [
        String::from("Patient/0192aeb0-0000-7000-8000-000000000001"),
        String::from("Patient/0192aeb0-0000-7000-8000-000000000002"),
    ];
    let search = SecretString::from(format!(
        "{BASE}/Patient?family=Proefsma&birthdate=1970-01-01"
    ));
    for (kind, profile, example) in [
        (Search::Query, "Query", "Query"),
        (Search::Match, "Match", "Match"),
    ] {
        let definition = vendored(
            "ihe-pdqm",
            &format!("package/StructureDefinition-IHE.PDQm.{profile}.Audit.Supplier.json"),
        );
        let records = pdq_search(&call(None), kind, &search, &persons, Outcome::Success);
        assert_eq!(
            2,
            records.len(),
            "BALP 1.1.4 §1:52.4.1.6: one record per patient"
        );
        for (record, person) in records.iter().map(written).zip(&persons) {
            holds_to(&record, &definition);
            like(
                &record,
                &vendored(
                    "ihe-pdqm",
                    &format!("package/example/AuditEvent-ex-auditPdqm{example}-supplier.json"),
                ),
            );
            let patient = entity(&record, "1", "1").expect("the patient entity");
            assert_eq!(person.as_str(), patient["what"]["reference"]);
        }
        let none = pdq_search(&call(None), kind, &search, &[], Outcome::Success);
        assert_eq!(1, none.len(), "a search that found no one");
        let record = written(&none[0]);
        holds_to(&record, &definition);
        assert!(entity(&record, "1", "1").is_none(), "no patient was found");
    }
    for (action, pattern) in [
        (StewardAction::View, "PatientRead"),
        (StewardAction::Decide, "PatientUpdate"),
    ] {
        let record = written(&steward(
            &call(Some("steward-ada")),
            action,
            "Patient/0192aeb0-0000-7000-8000-000000000001",
            Outcome::Success,
        ));
        holds_to(
            &record,
            &vendored(
                "ihe-balp",
                &format!("package/StructureDefinition-IHE.BasicAudit.{pattern}.json"),
            ),
        );
        assert!(
            agent_codes(&record).contains(&(
                String::from("http://terminology.hl7.org/CodeSystem/v3-ParticipationType"),
                String::from("IRCP")
            )),
            "{pattern}: the steward is the user agent"
        );
    }
}

#[test]
fn a_backend_client_acting_for_itself_is_recorded_with_no_user_agent() {
    let record = written(&pix_query(
        &call(None),
        SecretString::from("GET"),
        &Identifier {
            system: String::from(DOMAIN),
            value: SecretString::from("MRN-SYNTHETIC-2"),
        },
        Outcome::MinorFailure,
    ));
    assert!(!agent_codes(&record).contains(&(
        String::from("http://terminology.hl7.org/CodeSystem/v3-ParticipationType"),
        String::from("IRCP")
    )));
    let client = record["agent"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| code(&agent["type"]["coding"][0]).1 == "110153")
        .expect("the client agent");
    assert_eq!("east-lab", client["who"]["display"]);
    assert_eq!("192.0.2.7", client["network"]["address"]);
    assert_eq!("4", record["outcome"], "a minor failure");
}

/// The `[audit]` settings of a spool under `directory`, delivering to
/// `url` when given.
fn audit_settings(directory: &Path, url: Option<&str>, max_events: usize) -> AuditSettings {
    AuditSettings {
        url: url.map(|url| url::Url::parse(url).expect("a URL")),
        spool_dir: directory.to_path_buf(),
        spool_max_bytes: 1 << 20,
        spool_max_events: max_events,
        spool_write_timeout: Duration::from_secs(2),
        timeout: Duration::from_secs(2),
        retry_max: Duration::from_millis(200),
    }
}

/// A record naming the synthetic BSN.
fn bsn_record() -> Exchange {
    let bsn = bsn();
    pix_query(
        &call(None),
        raw_request(
            &url::Url::parse(&format!(
                "{BASE}/Patient/$ihe-pix?sourceIdentifier={BSN_SYSTEM}%7C{bsn}"
            ))
            .expect("a URL"),
            None,
        ),
        &Identifier {
            system: String::from(BSN_SYSTEM),
            value: SecretString::from(bsn),
        },
        Outcome::Success,
    )
}

fn base_url() -> url::Url {
    url::Url::parse("https://pix.example.org").expect("a URL")
}

#[test]
fn a_spool_directory_open_to_other_users_or_unwritable_refuses_the_start() {
    let parent = tempfile::tempdir().expect("a temporary directory");
    let exposed = parent.path().join("exposed");
    std::fs::create_dir_all(&exposed).expect("the directory is made");
    std::fs::set_permissions(&exposed, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let error = Audit::open(&audit_settings(&exposed, None, 10), &base_url())
        .expect_err("a spool others may read is refused");
    assert!(
        matches!(error, OpenError::Spool(SpoolError::Exposed(_))),
        "{error:?}"
    );
    let unwritable = parent.path().join("unwritable");
    std::fs::create_dir_all(&unwritable).expect("the directory is made");
    std::fs::create_dir_all(unwritable.join(QUARANTINE)).expect("the quarantine is made");
    std::fs::set_permissions(
        unwritable.join(QUARANTINE),
        std::fs::Permissions::from_mode(0o700),
    )
    .expect("chmod");
    std::fs::set_permissions(&unwritable, std::fs::Permissions::from_mode(0o500)).expect("chmod");
    let refused = Audit::open(&audit_settings(&unwritable, None, 10), &base_url());
    std::fs::set_permissions(&unwritable, std::fs::Permissions::from_mode(0o700))
        .expect("chmod back");
    assert!(
        matches!(refused, Err(OpenError::Spool(_))),
        "a spool the server cannot write is refused: {refused:?}"
    );
}

#[tokio::test]
async fn a_full_spool_refuses_every_audited_request_with_503_and_readiness_names_it() {
    let issuer = Issuer::start().await;
    let secrets = Secrets::new();
    let settings = auth_settings(&issuer, &secrets);
    let backend = crate::auth::backend(&settings);
    let spool = tempfile::tempdir().expect("a temporary directory");
    let audit = Audit::open(
        &audit_settings(&spool.path().join("spool"), None, 2),
        &base_url(),
    )
    .expect("the spool opens");
    let smart = Arc::new(
        Smart::discover(settings.auth, &settings.domains)
            .await
            .expect("the authorization server is read"),
    );
    let health = Registry::default().and(Arc::new(Headroom(audit.clone())));
    let lifecycle = crate::support::serving();
    let app = router(
        &settings.server,
        Health::new(health.clone(), lifecycle.clone()),
        Some(fhir::routes(
            &settings.server,
            Mounts::all(),
            &smart,
            &health,
            &lifecycle,
            &audit,
            &backend,
        )),
        RequestLog::default(),
        None,
    );
    let token = bearer(&issuer.token("east-lab", EVERY_SCOPE));
    let admitted = send(&app, Method::POST, "/fhir/Patient/$ihe-pix", Some(&token)).await;
    assert_eq!(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        admitted.status,
        "room in the spool: the Manager reads the request, which posts no FHIR body"
    );
    let ready = send(&app, Method::GET, "/health/readiness", None).await;
    assert_eq!(StatusCode::OK, ready.status, "{}", ready.body);
    audit
        .record(vec![bsn_record(), bsn_record()])
        .await
        .expect("two records fill the spool");
    for (method, path) in [
        (Method::POST, "/fhir/Patient/$ihe-pix"),
        (Method::GET, "/fhir/Patient"),
        (Method::POST, "/operator/persons/p-1/reveal"),
    ] {
        let answer = send(&app, method, path, Some(&token)).await;
        assert_eq!(StatusCode::SERVICE_UNAVAILABLE, answer.status, "{path}");
        let outcome = answer.json();
        assert_eq!("transient", outcome["issue"][0]["code"], "{path}");
        assert_eq!(UNAUDITED, outcome["issue"][0]["diagnostics"], "{path}");
    }
    let dependencies = send(&app, Method::GET, "/operator/dependencies", Some(&token)).await;
    assert_eq!(
        StatusCode::OK,
        dependencies.status,
        "an unaudited read still answers"
    );
    let ready = send(&app, Method::GET, "/health/readiness", None).await;
    assert_eq!(StatusCode::SERVICE_UNAVAILABLE, ready.status);
    assert_eq!(
        serde_json::json!([{ "name": "audit_spool", "state": "down" }]),
        ready.json()["indicators"],
        "the open route names the indicator and never its detail"
    );
}

/// The stored records of the spool at `directory`, quarantined ones apart.
fn stored(directory: &Path) -> Vec<PathBuf> {
    files(directory)
        .into_iter()
        .filter(|path| path.parent() == Some(directory))
        .collect()
}

/// Every file under `directory`, recursively.
fn files(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).expect("the directory reads") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}

/// Waits until `condition` holds, for at most ten seconds.
async fn eventually(condition: impl Fn() -> bool) -> bool {
    for _ in 0..200 {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    condition()
}

#[tokio::test]
async fn a_delivered_record_is_removed_and_a_refused_one_quarantined() {
    let repository = MockServer::start().await;
    let spool = tempfile::tempdir().expect("a temporary directory");
    let directory = spool.path().join("spool");
    let audit = Audit::open(
        &audit_settings(&directory, Some(&format!("{}/fhir", repository.uri())), 10),
        &base_url(),
    )
    .expect("the spool and the feed open");
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/fhir/AuditEvent"))
        .respond_with(ResponseTemplate::new(201))
        .up_to_n_times(1)
        .mount(&repository)
        .await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/fhir/AuditEvent"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&repository)
        .await;
    let draining = audit.drain().expect("a repository is configured");
    audit
        .record(vec![bsn_record()])
        .await
        .expect("the record is stored");
    assert!(
        eventually(
            || stored(&directory).is_empty() && files(&directory.join(QUARANTINE)).is_empty()
        )
        .await,
        "the delivered record is removed"
    );
    // The forwarder removes a delivered record before it counts the delivery.
    assert!(
        eventually(|| {
            audit
                .upstream()
                .expect("the repository is named")
                .last
                .map(|last| last.state)
                == Some(ferrohealth_health::document::State::Up)
        })
        .await,
        "the repository reads up once the delivery is counted"
    );
    audit
        .record(vec![bsn_record()])
        .await
        .expect("the record is stored");
    assert!(
        eventually(
            || stored(&directory).is_empty() && files(&directory.join(QUARANTINE)).len() == 1
        )
        .await,
        "the refused record is quarantined"
    );
    draining.abort();
    let posted = repository.received_requests().await.unwrap_or_default();
    assert_eq!(2, posted.len());
    assert_eq!(
        Some("application/fhir+json"),
        posted[0]
            .headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
    );
}

#[tokio::test]
async fn the_audit_record_is_the_only_place_an_identifier_reaches_the_disk_or_the_log() {
    let bsn = bsn();
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let state = tempfile::tempdir().expect("a temporary directory");
    let directory = state.path().join("spool");
    let audit =
        Audit::open(&audit_settings(&directory, None, 10), &base_url()).expect("the spool opens");
    let exchange = bsn_record();
    assert!(
        !format!("{exchange:?}").contains(&bsn),
        "an exchange's Debug"
    );
    audit
        .record(vec![exchange])
        .await
        .expect("the record is stored");
    let holding: Vec<PathBuf> = files(state.path())
        .into_iter()
        .filter(|path| {
            std::fs::read(path).is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(&bsn))
        })
        .collect();
    assert_eq!(
        stored(&directory),
        holding,
        "only the spooled record holds the BSN"
    );
    let record: Value =
        serde_json::from_slice(&std::fs::read(&holding[0]).expect("the record reads"))
            .expect("JSON");
    assert_eq!("AuditEvent", record["resourceType"]);
    let mode = std::fs::metadata(&holding[0])
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(0o600, mode, "ITI TF-2 §3.20.4.1.1 spool: the owner alone");
    let log = String::from_utf8(captured.0.lock().expect("the buffer").clone()).expect("UTF-8");
    assert!(!log.contains(&bsn), "the log holds the BSN");
}
