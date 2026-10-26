// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The XCPD Responding Gateway over PostgreSQL 18 answering IHE's own
//! example request (#228), behind `FERROPIX_E2E`: the request IHE publishes
//! as ITI-55 implementation material (ITI TF-2 Appendix W), vendored under
//! `docs/specs/ihe-xcpd-examples/`, finds the patient its shared identifier
//! names, and is answered with Case 1.
//!
//! The patient is IHE's fictional test patient of that example.

use std::path::PathBuf;

use ferropix_server::config::section::domain::{DomainKind, DomainSeed};
use ferropix_server::config::section::xcpd::Xcpd;
use ferropix_testkit::containers::e2e_enabled;
use http::StatusCode;

use super::decisions::fed;
use super::pixm::{Harness, SOURCE};
use super::xcpd::{SOAP, events, gateway_with, post};

/// The assigning authority of the example's first `LivingSubjectId`.
const AUTHORITY: &str = "1.2.840.114350.1.13.99997.2.3412";

/// The initiating community the example's sender names.
const INITIATING: &str = "1.2.3.928.955";

/// The text of IHE's example `name`.
fn example(name: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "../../docs/specs/ihe-xcpd-examples",
        name,
    ]
    .iter()
    .collect();
    std::fs::read_to_string(&path).expect("an example under docs/specs/ihe-xcpd-examples")
}

#[tokio::test]
async fn ihes_example_request_finds_the_patient_its_identifier_names() {
    if !e2e_enabled() {
        return;
    }
    let system = format!("urn:oid:{AUTHORITY}");
    let harness = Harness::start(vec![DomainSeed {
        system: system.clone(),
        name: String::from("IHE example authority"),
        kind: DomainKind::Local,
        source_client_id: SOURCE.to_owned(),
        sensitive: true,
    }])
    .await;
    let patient = serde_json::json!({
        "resourceType": "Patient",
        "identifier": [{"system": system, "value": "1234"}],
        "name": [{"family": "Jones", "given": ["Jimmy"]}],
        "gender": "male",
        "birthDate": "1963-08-04"
    });
    fed(
        &harness,
        &system,
        "1234",
        &serde_json::from_value(patient).expect("a Patient"),
    )
    .await;
    let xcpd = Xcpd {
        enabled: true,
        home_community_id: String::from("urn:oid:2.999.60.1"),
        device: String::from("1.2.840.114350.1.13.999.234"),
        domains: vec![system.clone()],
        initiating_communities: vec![format!("urn:oid:{INITIATING}")],
        ..Xcpd::default()
    };
    let app = gateway_with(
        &harness,
        &harness.matching,
        xcpd.resolve().expect("resolves").expect("on"),
    );
    let (status, answer) = post(
        &app,
        SOAP,
        example("XCPDCrossGatewayPatientDiscoveryRequest.xml"),
    )
    .await;
    assert_eq!(StatusCode::OK, status, "{answer}");
    assert_eq!(1, events(&answer), "Case 1: {answer}");
    assert!(
        answer.contains(&format!(r#"root="{AUTHORITY}" extension="1234""#)),
        "{answer}"
    );
    assert!(
        answer.contains(
            "<wsa:RelatesTo>urn:uuid:a02ca8cd-86fa-4afc-a27c-16c183b2055</wsa:RelatesTo>"
        ),
        "{answer}"
    );
}
