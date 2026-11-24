// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The EHDS identifier types: one domain of each type of the value set of the
//! draft `EEHRxF` Annexes, Table 26 `EHDSPatient.identifier.type`, one patient
//! fed in each, and the identifier the PDQm Supplier serves for it carries
//! its domain's type: a coding of the HL7 Identifier Type for a v2-0203 code,
//! the code as `type.text` for an EHDS code no code system holds.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the answers are read as JSON"
)]

use ferropix_conformance::{record, verdict};
use ferropix_operator_api::patient::IdentifierType;
use ferropix_server::config::section::domain::{DomainKind, DomainSeed};
use ferropix_server::config::section::matching::Matching;
use ferropix_server::config::section::profiles::Deprecated;
use ferropix_server::store::{Job, Store};
use ferropix_testkit::containers::{e2e_enabled, postgres};
use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES};
use http::{Method, StatusCode};
use serde_json::{Value, json};

use crate::e2e::feeds::{SOURCE, feeder};
use crate::e2e::supplier::{Fed, feed, over, request};
use crate::e2e::{app_role, fresh_key, settings};

/// The HL7 Identifier Type code system.
const V2_0203: &str = "http://terminology.hl7.org/CodeSystem/v2-0203";

/// The synthetic system of the domain of each type, under the OID arc 2.999
/// ITU-T X.660 reserves for examples.
const SYSTEMS: [&str; 12] = [
    "urn:oid:2.999.7.1",
    "urn:oid:2.999.7.2",
    "urn:oid:2.999.7.3",
    "urn:oid:2.999.7.4",
    "urn:oid:2.999.7.5",
    "urn:oid:2.999.7.6",
    "urn:oid:2.999.7.7",
    "urn:oid:2.999.7.8",
    "urn:oid:2.999.7.9",
    "urn:oid:2.999.7.10",
    "urn:oid:2.999.7.11",
    "urn:oid:2.999.7.12",
];

/// The `Identifier.type` the domain of `kind` gives its identifiers.
fn expected(kind: IdentifierType) -> Value {
    match kind.v2_display() {
        Some(display) => {
            json!({"coding": [{"system": V2_0203, "code": kind.code(), "display": display}]})
        }
        None => json!({"text": kind.code()}),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn conformance_every_served_identifier_carries_its_domain_type() {
    if !e2e_enabled() {
        return;
    }
    let seeds: Vec<DomainSeed> = IdentifierType::ALL
        .into_iter()
        .zip(SYSTEMS)
        .map(|(kind, system)| DomainSeed {
            system: system.to_owned(),
            name: format!("Synthetic {kind} domain"),
            kind: DomainKind::Local,
            identifier_type: kind,
            source_client_id: SOURCE.to_owned(),
            sensitive: false,
        })
        .collect();
    let postgres = postgres().await.expect("PostgreSQL starts");
    let url = app_role(&postgres).await;
    let store = Store::start(&settings(&url, &fresh_key(), true), &seeds, Job::Migrate)
        .await
        .expect("the store starts");
    let (app, issuer, _spool, _secrets) =
        over(&store, Deprecated::Inactive, &Matching::default()).await;
    let token = crate::auth::bearer(&issuer.token("east-lab", "system/Patient.rs"));
    let fed = feeder(&store);
    let mut cases = Vec::new();
    for (index, (kind, system)) in IdentifierType::ALL.into_iter().zip(SYSTEMS).enumerate() {
        let value = format!("T-{}", 700_001 + index);
        let family = FAMILY_NAMES[index % FAMILY_NAMES.len()];
        let given = GIVEN_NAMES[index % GIVEN_NAMES.len()];
        let mut patient = Fed::new(system, &value, family, given);
        patient.birth = format!("1950-01-{:02}", index + 1);
        let person = feed(&fed, system, &value, &patient.body()).await;
        let answered = request(
            &app,
            Method::GET,
            &format!("/fhir/Patient/{person}"),
            &[("authorization", &token)],
            Vec::new(),
        )
        .await;
        cases.push(
            verdict(kind.code().to_owned(), async move {
                assert_eq!(StatusCode::OK, answered.status, "{}", answered.body);
                let body: Value = serde_json::from_str(&answered.body).expect("JSON");
                let identifier = body["identifier"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|identifier| identifier["system"] == system)
                    .unwrap_or_else(|| panic!("no identifier in {system}"));
                assert_eq!(expected(kind), identifier["type"], "{system}");
            })
            .await,
        );
    }
    record(
        "ehds-identifier-types",
        "The EHDS identifier types of the draft EEHRxF Annexes, Table 26, one served identifier of each",
        cases,
    );
}
