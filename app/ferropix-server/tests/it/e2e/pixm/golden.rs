// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The bytes of ITI-83 answers, held to the ones the seven-statement read
//! wrote before the query became one statement (#715): the identifiers in
//! their order, the pseudonyms, `targetSystem` by GET and by POST, JSON and
//! XML, and the refusals (PIXm 3.1.0 §2:3.83.4.2.2). Every id and pseudonym
//! is masked by its order of appearance, so the bytes compare across runs.

use std::fmt::Write as _;

use ferropix_server::config::section::pseudonym::PseudonymSeed;
use ferropix_server::store::pseudonym;
use ferropix_testkit::containers::e2e_enabled;
use ferropix_testkit::synthetic::{HOSPITAL_DOMAIN, LABORATORY_DOMAIN};
use http::{Method, header};
use uuid::Uuid;

use super::{Harness, LAB, feed_both, pix_parameters, pix_uri, synthetic_seeds};
use crate::auth::bearer;
use crate::e2e::BSN_SYSTEM;

/// The clinical pseudonymisation domain.
const NAMESPACE: &str = "urn:oid:2.999.7.1";

/// The client the clinical domain's pseudonyms are minted for.
const EHR: &str = "synthetic-ehr";

/// The answers the seven-statement read wrote, masked.
const GOLDEN: &str = r#"--- lab, every domain
200 {"parameter":[{"name":"targetIdentifier","valueIdentifier":{"system":"http://fhir.nl/fhir/NamingSystem/bsn","value":"999990007"}},{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.1.1","value":"H-100001"}},{"name":"targetId","valueReference":{"reference":"Patient/{uuid-1}"}}],"resourceType":"Parameters"}
--- lab, hospital and BSN
200 {"parameter":[{"name":"targetIdentifier","valueIdentifier":{"system":"http://fhir.nl/fhir/NamingSystem/bsn","value":"999990007"}},{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.1.1","value":"H-100001"}},{"name":"targetId","valueReference":{"reference":"Patient/{uuid-1}"}}],"resourceType":"Parameters"}
--- lab, in XML
200 <Parameters xmlns="http://hl7.org/fhir"><parameter><name value="targetIdentifier"/><valueIdentifier><system value="http://fhir.nl/fhir/NamingSystem/bsn"/><value value="999990007"/></valueIdentifier></parameter><parameter><name value="targetIdentifier"/><valueIdentifier><system value="urn:oid:2.999.1.1"/><value value="H-100001"/></valueIdentifier></parameter><parameter><name value="targetId"/><valueReference><reference value="Patient/{uuid-1}"/></valueReference></parameter></Parameters>
--- BSN, every domain
200 {"parameter":[{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.1.1","value":"H-100001"}},{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.1.2","value":"L-200001"}},{"name":"targetId","valueReference":{"reference":"Patient/{uuid-1}"}}],"resourceType":"Parameters"}
--- EHR, the clinical domain, minted
200 {"parameter":[{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.7.1","value":"{uuid-2}"}},{"name":"targetId","valueReference":{"reference":"Patient/{uuid-1}"}}],"resourceType":"Parameters"}
--- EHR, the clinical domain, held
200 {"parameter":[{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.7.1","value":"{uuid-2}"}},{"name":"targetId","valueReference":{"reference":"Patient/{uuid-1}"}}],"resourceType":"Parameters"}
--- EHR, every domain
200 {"parameter":[{"name":"targetIdentifier","valueIdentifier":{"system":"http://fhir.nl/fhir/NamingSystem/bsn","value":"999990007"}},{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.1.2","value":"L-200001"}},{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.7.1","value":"{uuid-2}"}},{"name":"targetId","valueReference":{"reference":"Patient/{uuid-1}"}}],"resourceType":"Parameters"}
--- EHR, the clinical domain and the lab
200 {"parameter":[{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.1.2","value":"L-200001"}},{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.7.1","value":"{uuid-2}"}},{"name":"targetId","valueReference":{"reference":"Patient/{uuid-1}"}}],"resourceType":"Parameters"}
--- consumer, the clinical domain
200 {"parameter":[{"name":"targetId","valueReference":{"reference":"Patient/{uuid-1}"}}],"resourceType":"Parameters"}
--- an unknown target
403 {"issue":[{"code":"code-invalid","diagnostics":"targetSystem not found","severity":"error"}],"resourceType":"OperationOutcome"}
--- an unknown identifier
404 {"issue":[{"code":"not-found","diagnostics":"sourceIdentifier Patient Identifier not found","severity":"error"}],"resourceType":"OperationOutcome"}
--- an unknown domain
400 {"issue":[{"code":"code-invalid","diagnostics":"sourceIdentifier Assigning Authority not found","severity":"error"}],"resourceType":"OperationOutcome"}
--- lab, hospital, posted
200 {"parameter":[{"name":"targetIdentifier","valueIdentifier":{"system":"urn:oid:2.999.1.1","value":"H-100001"}},{"name":"targetId","valueReference":{"reference":"Patient/{uuid-1}"}}],"resourceType":"Parameters"}
"#;

/// Returns `text` with each UUID replaced by `{uuid-N}`, `N` its order of
/// first appearance.
fn masked(text: &str) -> String {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = String::with_capacity(text.len());
    let mut skip_to = 0;
    for (at, char) in text.char_indices() {
        if at < skip_to {
            continue;
        }
        let uuid = text
            .get(at..at + 36)
            .filter(|head| Uuid::try_parse(head).is_ok());
        let Some(head) = uuid else {
            out.push(char);
            continue;
        };
        let index = seen
            .iter()
            .position(|known| *known == head)
            .unwrap_or_else(|| {
                seen.push(head);
                seen.len() - 1
            });
        write!(out, "{{uuid-{}}}", index + 1).expect("a String takes it");
        skip_to = at + 36;
    }
    out
}

/// One ITI-83 by GET as `token`, the answer as `status body`.
async fn get(harness: &Harness, token: &str, uri: &str, accept: &str) -> String {
    let answer = harness
        .send(
            Method::GET,
            uri,
            Some(token),
            &[(header::ACCEPT, accept)],
            None,
        )
        .await;
    format!("{} {}", answer.status.as_u16(), answer.body)
}

#[tokio::test]
async fn the_iti_83_answers_keep_their_bytes() {
    if !e2e_enabled() {
        return;
    }
    let harness = Harness::start(synthetic_seeds()).await;
    pseudonym::seed(
        &harness.store,
        &[PseudonymSeed {
            namespace: NAMESPACE.to_owned(),
            name: String::from("Clinical domain"),
            consumer_client_id: Some(EHR.to_owned()),
        }],
    )
    .await
    .expect("the domain is seeded");
    let mut joined = String::new();
    for (label, answer) in answers(&harness).await {
        writeln!(joined, "--- {label}\n{answer}").expect("a String takes it");
    }
    assert_eq!(GOLDEN, masked(&joined));
}

/// Asks every ITI-83 the golden holds, each answer as `status body` under
/// its label.
async fn answers(harness: &Harness) -> Vec<(&'static str, String)> {
    let (hospital, bsn) = feed_both(harness).await;
    let consumer = harness.consumer();
    let ehr = bearer(&harness.issuer.token(EHR, "system/Patient.rs"));
    let (json, xml) = ("application/fhir+json", "application/fhir+xml");
    let lab = |targets: &[&str]| pix_uri(LABORATORY_DOMAIN, LAB, targets);
    let held = |targets: &[&str]| pix_uri(HOSPITAL_DOMAIN, &hospital, targets);
    let asked: Vec<(&'static str, &str, String, &str)> = vec![
        ("lab, every domain", &consumer, lab(&[]), json),
        (
            "lab, hospital and BSN",
            &consumer,
            lab(&[HOSPITAL_DOMAIN, BSN_SYSTEM]),
            json,
        ),
        ("lab, in XML", &consumer, lab(&[]), xml),
        (
            "BSN, every domain",
            &consumer,
            pix_uri(BSN_SYSTEM, &bsn, &[]),
            json,
        ),
        (
            "EHR, the clinical domain, minted",
            &ehr,
            held(&[NAMESPACE]),
            json,
        ),
        (
            "EHR, the clinical domain, held",
            &ehr,
            held(&[NAMESPACE]),
            json,
        ),
        ("EHR, every domain", &ehr, held(&[]), json),
        (
            "EHR, the clinical domain and the lab",
            &ehr,
            held(&[LABORATORY_DOMAIN, NAMESPACE]),
            json,
        ),
        (
            "consumer, the clinical domain",
            &consumer,
            held(&[NAMESPACE]),
            json,
        ),
        (
            "an unknown target",
            &consumer,
            lab(&["urn:oid:2.999.9.9"]),
            json,
        ),
        (
            "an unknown identifier",
            &consumer,
            pix_uri(LABORATORY_DOMAIN, "L-999999", &[]),
            json,
        ),
        (
            "an unknown domain",
            &consumer,
            pix_uri("urn:oid:2.999.9.8", LAB, &[]),
            json,
        ),
    ];
    let mut answers = Vec::with_capacity(asked.len() + 1);
    for (label, token, uri, accept) in asked {
        answers.push((label, get(harness, token, &uri, accept).await));
    }
    let posted = harness
        .send(
            Method::POST,
            "/fhir/Patient/$ihe-pix",
            Some(&consumer),
            &[(header::CONTENT_TYPE, json), (header::ACCEPT, json)],
            Some(pix_parameters(LABORATORY_DOMAIN, LAB, &[HOSPITAL_DOMAIN])),
        )
        .await;
    answers.push((
        "lab, hospital, posted",
        format!("{} {}", posted.status.as_u16(), posted.body),
    ));
    answers
}
