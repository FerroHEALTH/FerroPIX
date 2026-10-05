// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The recorders the client tests audit through, and the check that holds a
//! written record to its vendored audit profile, the BALP pattern beneath it
//! and the profile's own example.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the records and the vendored profiles are read as JSON values"
)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;

use ihe_iti::balp::{AuditError, AuditRecorder, Exchange};
use ihe_iti::user::{OnBehalfOf, PurposeOfUse, User};
use secrecy::ExposeSecret as _;
use serde_json::Value;

/// The canonical URL prefix of the BALP patterns.
const BALP: &str = "https://profiles.ihe.net/ITI/BALP/StructureDefinition/";

/// A recorder that keeps every exchange it is given.
#[derive(Default)]
pub(crate) struct Kept(Mutex<Vec<Exchange>>);

impl Kept {
    /// The exchanges kept so far, in order.
    pub(crate) fn taken(&self) -> Vec<Exchange> {
        std::mem::take(&mut *self.0.lock().expect("the kept exchanges"))
    }
}

#[async_trait::async_trait]
impl AuditRecorder for Kept {
    async fn record(&self, exchange: Exchange) -> Result<(), AuditError> {
        self.0.lock().expect("the kept exchanges").push(exchange);
        Ok(())
    }
}

/// A recorder that accepts nothing, as a full spool does not.
pub(crate) struct Refusing;

#[derive(Debug, thiserror::Error)]
#[error("the spool is full")]
struct Full;

#[async_trait::async_trait]
impl AuditRecorder for Refusing {
    async fn record(&self, _exchange: Exchange) -> Result<(), AuditError> {
        Err(AuditError(Box::new(Full)))
    }
}

/// A recorder that never accepts, as a spool whose disk stalled does not.
pub(crate) struct Stalled;

#[async_trait::async_trait]
impl AuditRecorder for Stalled {
    async fn record(&self, _exchange: Exchange) -> Result<(), AuditError> {
        std::future::pending().await
    }
}

/// The `AuditEvent` `exchange` is written as, read back as JSON.
pub(crate) fn written(exchange: &Exchange) -> Value {
    let record = exchange
        .audit_event(&super::observer())
        .expect("a record")
        .into_bytes();
    let bytes = record.expose_secret();
    serde_json::from_slice::<fhir_types::r4::audit_event::AuditEvent>(bytes)
        .expect("the record reads back as an R4 AuditEvent");
    serde_json::from_slice(bytes).expect("JSON")
}

/// `text` base64-decoded, as UTF-8.
pub(crate) fn base64_decoded(text: &str) -> String {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text)
        .expect("base64");
    String::from_utf8(bytes).expect("UTF-8")
}

/// A vendored file, by its corpus directory and its path in it.
pub(crate) fn vendored(corpus: &str, file: &str) -> Value {
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

/// The codings of an `agent.type`.
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
        .map(|entity| {
            (
                entity["type"]["code"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                entity["role"]["code"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect()
}

/// Holds `record` to every fixed value and pattern the differentials of
/// `profile` and the BALP patterns beneath it fix: `meta.profile`, `type`,
/// each `subtype` slice, `action`, `outcome`, each required agent slice's
/// type, and each required entity slice's type and role.
pub(crate) fn holds_to(record: &Value, profile: &Value) {
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
                ("AuditEvent.outcome", _, Some(outcome)) => {
                    assert_eq!(record["outcome"].as_str(), Some(outcome), "{url}: outcome");
                }
                (id, Some(pattern), _)
                    if id.starts_with("AuditEvent.subtype:") && !id.contains('.') =>
                {
                    assert!(
                        subtypes.contains(&code(pattern)),
                        "{url}: subtype {id} {pattern} in {subtypes:?}"
                    );
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
                        "{url}: {slice} type {coding} in {agents:?}"
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

/// The issuer of the synthetic user's token.
pub(crate) const ISSUER: &str = "https://issuer.example.test";

/// The synthetic user's `sub`, unlike any other text.
pub(crate) const SUBJECT: &str = "Qz7-clinician-31";

/// The synthetic user's `client_id`, unlike any other text.
pub(crate) const CLIENT: &str = "Qz7-client-32";

/// The audience the synthetic user's token names.
pub(crate) const AUDIENCE: &str = "urn:example:gateway-under-test";

/// The HL7 v3 `ActReason` code system of the synthetic purpose of use.
pub(crate) const ACT_REASON: &str = "http://terminology.hl7.org/CodeSystem/v3-ActReason";

/// A synthetic user who asked for an exchange through their OAuth token, for
/// the purpose of use `TREAT`.
pub(crate) fn user() -> OnBehalfOf {
    OnBehalfOf::User(
        User::new(ISSUER.to_owned(), SUBJECT.to_owned(), CLIENT.to_owned())
            .with_audience(Some(AUDIENCE.to_owned()))
            .with_purposes(vec![PurposeOfUse {
                system: Some(ACT_REASON.to_owned()),
                code: "TREAT".to_owned(),
            }]),
    )
}

/// The agents of `record` whose type is `(system, code)`.
fn agents_of<'a>(record: &'a Value, system: &str, code: &str) -> Vec<&'a Value> {
    record["agent"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|agent| {
            agent["type"]["coding"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|coding| coding["system"] == system && coding["code"] == code)
        })
        .collect()
}

/// Holds `record` to the `agent:user` slice of BALP Query, which Patient
/// Query and every transaction profile built on either inherit: exactly one agent of the slice's fixed type,
/// with a `who`, `requestor` as fixed, and no `network` or `media`; and
/// checks it names [`user`] as BALP 1.1.4 §3:5.7.5.4 maps the token: `iss`
/// and `sub` in `who.identifier`, the purpose of use in `purposeOfUse`, and
/// `client_id` in the `who.identifier.value` of one Application agent.
pub(crate) fn names_the_user(record: &Value) {
    let definition = vendored(
        "ihe-balp",
        "package/StructureDefinition-IHE.BasicAudit.Query.json",
    );
    let elements = definition["differential"]["element"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let element = |id: &str| {
        elements
            .iter()
            .find(|element| element["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("BALP Query defines {id}"))
    };
    let typed = element("AuditEvent.agent:user.type");
    let coding = &typed["patternCodeableConcept"]["coding"][0];
    let users = agents_of(
        record,
        coding["system"].as_str().expect("a system"),
        coding["code"].as_str().expect("a code"),
    );
    let [person] = users.as_slice() else {
        panic!("one agent:user, got {}", users.len());
    };
    assert_eq!(
        element("AuditEvent.agent:user")["max"],
        "1",
        "BALP Query: agent:user is 0..1"
    );
    assert!(person["who"].is_object(), "agent:user.who is 1..1");
    assert_eq!(
        person["requestor"],
        element("AuditEvent.agent:user.requestor")["patternBoolean"],
        "agent:user.requestor"
    );
    for absent in ["network", "media"] {
        assert_eq!(
            element(&format!("AuditEvent.agent:user.{absent}"))["max"],
            "0"
        );
        assert!(person[absent].is_null(), "agent:user.{absent} is 0..0");
    }
    assert_eq!(person["who"]["identifier"]["system"], ISSUER, "iss");
    assert_eq!(person["who"]["identifier"]["value"], SUBJECT, "sub");
    assert_eq!(
        person["purposeOfUse"][0]["coding"][0]["system"], ACT_REASON,
        "purpose_of_use"
    );
    assert_eq!(person["purposeOfUse"][0]["coding"][0]["code"], "TREAT");
    let applications = agents_of(
        record,
        "http://dicom.nema.org/resources/ontology/DCM",
        "110150",
    );
    let [application] = applications.as_slice() else {
        panic!("one Application agent, got {}", applications.len());
    };
    assert_eq!(
        application["who"]["identifier"]["value"], CLIENT,
        "client_id"
    );
    assert_eq!(application["requestor"], false);
}

/// Checks `record` names no user: no `IRCP` agent and no Application agent,
/// as the BALP examples of an event no user caused do.
pub(crate) fn names_no_user(record: &Value) {
    assert!(
        agents_of(
            record,
            "http://terminology.hl7.org/CodeSystem/v3-ParticipationType",
            "IRCP"
        )
        .is_empty(),
        "no agent:user"
    );
    assert!(
        agents_of(
            record,
            "http://dicom.nema.org/resources/ontology/DCM",
            "110150"
        )
        .is_empty()
            || record["meta"]["profile"][0]
                .as_str()
                .is_some_and(|profile| profile.contains("Delete")),
        "no Application agent beside a Delete pattern's own client"
    );
    let text = record.to_string();
    for value in [SUBJECT, CLIENT] {
        assert!(!text.contains(value), "the record names no user");
    }
}

/// Holds `record` to the profile's own example: the same type, subtypes,
/// action and system agent types, and the same entity types and roles,
/// leaving out the example's user agent, which a client of this crate is
/// told of none of. With `entities` false the entities are left out too, for
/// an example that names an entity its profile makes optional.
pub(crate) fn like(record: &Value, example: &Value, entities: bool) {
    let user_types: BTreeSet<(String, String)> = [
        (
            "http://terminology.hl7.org/CodeSystem/v3-ParticipationType",
            "IRCP",
        ),
        (
            "http://terminology.hl7.org/CodeSystem/v3-ParticipationType",
            "AUT",
        ),
        (
            "http://terminology.hl7.org/CodeSystem/v3-ParticipationType",
            "CST",
        ),
    ]
    .into_iter()
    .map(|(system, code)| (system.to_owned(), code.to_owned()))
    .collect();
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
    // NOTE: the mCSD Query and Updates examples leave out the action the profiles
    // fix; the profile, which `holds_to` checks, is the authority.
    if !example["action"].is_null() {
        assert_eq!(record["action"], example["action"], "action");
    }
    let system_agents: BTreeSet<_> = agent_codes(example)
        .difference(&user_types)
        .cloned()
        .collect();
    assert_eq!(agent_codes(record), system_agents, "agent types");
    if entities {
        assert_eq!(entity_codes(record), entity_codes(example), "entities");
    }
}
