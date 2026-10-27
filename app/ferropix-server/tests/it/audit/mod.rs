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
use std::path::PathBuf;
use std::sync::Arc;

use ferrohealth_smart::resource::{Access, Caller};
use ferropix_server::audit::records::operator::{StewardAction, steward};
use ferropix_server::audit::records::{Call, raw_request};
use ihe_atna::balp::{Exchange, NetworkAddress, Observer, Outcome, SOURCE_APPLICATION_SERVER};
use ihe_iti::pdqm::audit::{SupplierSearch, supplier_records};
use ihe_iti::pixm::audit::{FeedAction, manager_feed, manager_query};
use ihe_iti::pixm::identifier::SourceIdentifier;
use secrecy::{ExposeSecret as _, SecretString};
use serde_json::Value;

mod spool;

/// The canonical URL prefix of the BALP patterns.
const BALP: &str = "https://profiles.ihe.net/ITI/BALP/StructureDefinition/";

/// The FHIR base the records name as the endpoint.
pub(super) const BASE: &str = "https://pix.example.org/fhir";

/// The domain of the synthetic identifiers.
const DOMAIN: &str = "urn:oid:2.999.1.1";

/// The synthetic BSN system.
pub(super) const BSN_SYSTEM: &str = "http://fhir.nl/fhir/NamingSystem/bsn";

/// A synthetic BSN from the reserved range.
pub(super) fn bsn() -> String {
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
pub(super) fn call(user: Option<&str>) -> Call {
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
pub(super) fn code(coding: &Value) -> (String, String) {
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
    let source =
        SourceIdentifier::new(BSN_SYSTEM, SecretString::from(bsn.clone())).expect("an identifier");
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
        let record = written(&manager_query(
            &call(None).received(),
            raw,
            &source,
            Outcome::Success,
        ));
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
    let fed = SourceIdentifier::new(DOMAIN, SecretString::from("MRN-SYNTHETIC-1"))
        .expect("an identifier");
    for (action, profile, example) in [
        (FeedAction::Create, "Create", "create"),
        (FeedAction::Update, "Update", "update"),
        (FeedAction::Delete, "Delete", "delete"),
    ] {
        let record = written(&manager_feed(
            &call(None).received(),
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
        (SupplierSearch::Query, "Query", "Query"),
        (SupplierSearch::Match, "Match", "Match"),
    ] {
        let definition = vendored(
            "ihe-pdqm",
            &format!("package/StructureDefinition-IHE.PDQm.{profile}.Audit.Supplier.json"),
        );
        let records = supplier_records(
            &call(None).received(),
            kind,
            &search,
            &persons,
            Outcome::Success,
        );
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
        let none = supplier_records(&call(None).received(), kind, &search, &[], Outcome::Success);
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
    let record = written(&manager_query(
        &call(None).received(),
        SecretString::from("GET"),
        &SourceIdentifier::new(DOMAIN, SecretString::from("MRN-SYNTHETIC-2"))
            .expect("an identifier"),
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
