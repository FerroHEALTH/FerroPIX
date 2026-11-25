// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Every `Patient` the PDQm Supplier answers for the shared population's
//! active persons,
//! validated with `ferrobridge-validator` against the MyHealth@EU NCP API
//! Patient-minimal profile (`myhealth.eu.fhir.ncp-api` 9.1.1-ci) and the PDQm
//! Patient profile (PDQm 3.2.0 §2:3.78.4.2.2.1). An answer passes when
//! neither validation reports an error.

use std::path::{Path, PathBuf};

use ferrobridge_validator::issue::Severity;
use ferrobridge_validator::release::R4;
use ferrobridge_validator::set::PackageSet;
use ferrobridge_validator::validate::Validator;
use ferropix_conformance::{record, verdict};
use ferropix_server::config::section::matching::Matching;
use ferropix_server::config::section::profiles::Deprecated;
use ferropix_testkit::containers::e2e_enabled;
use http::{Method, StatusCode};
use std::sync::Arc;

use crate::e2e::supplier::{populate, request, serve};

/// The Patient-minimal profile of the MyHealth@EU NCP API.
const PATIENT_MINIMAL: &str =
    "http://fhir.ehdsi.eu/ncp-api/StructureDefinition/Patient-minimal-myhealtheu";

/// The PDQm Patient profile.
const PDQM_PATIENT: &str = "https://profiles.ihe.net/ITI/PDQm/StructureDefinition/IHE.PDQm.Patient";

/// The vendored package and its fetched closure, from the workspace root.
const PACKAGE: &str = "docs/specs/myhealtheu-ncp-api/myhealth.eu.fhir.ncp-api-9.1.1-ci.tgz";
const CLOSURE: &str = "docs/specs/myhealtheu-ncp-api/closure";

/// The package set: the vendored package and every tarball of its closure.
fn packages() -> PackageSet<R4> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let closure = root.join(CLOSURE);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&closure)
        .unwrap_or_else(|error| {
            panic!(
                "{}: {error}; run scripts/vendor/myhealtheu-ncp-api.sh --closure",
                closure.display()
            )
        })
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "tgz"))
        .collect();
    paths.sort();
    paths.push(root.join(PACKAGE));
    PackageSet::load(&paths).expect("the package closure loads")
}

#[tokio::test(flavor = "multi_thread")]
async fn conformance_the_pdqm_answers_hold_to_patient_minimal() {
    // NOTE: no specification governs this: our own design; the package closure is
    // fetched by scripts/checks/conformance.sh, which sets FERROPIX_CONFORMANCE_OUT.
    if !e2e_enabled() || std::env::var_os("FERROPIX_CONFORMANCE_OUT").is_none() {
        return;
    }
    let set = Arc::new(packages());
    let served = serve(Deprecated::Inactive, &Matching::default()).await;
    let population = populate(&served.started.store).await;
    let token = served.consumer();
    let mut cases = Vec::new();
    for (name, person) in [
        ("ada-hospital", population.ada_hospital),
        ("ada-laboratory", population.ada_laboratory),
        ("bram", population.bram),
        ("cato", population.cato),
    ] {
        let answered = request(
            &served.app,
            Method::GET,
            &format!("/fhir/Patient/{person}"),
            &[("authorization", &token)],
            Vec::new(),
        )
        .await;
        let set = Arc::clone(&set);
        cases.push(
            verdict(format!("Patient/{name}"), async move {
                assert_eq!(StatusCode::OK, answered.status, "{}", answered.body);
                let patient: fhir_types::codec::Value =
                    serde_json::from_str(&answered.body).expect("a JSON Patient");
                let validator = Validator::new(&set);
                for profile in [PATIENT_MINIMAL, PDQM_PATIENT] {
                    let report = validator
                        .validate_against(&patient, profile)
                        .expect("the profile resolves");
                    let errors: Vec<String> = report
                        .issues()
                        .iter()
                        .filter(|issue| issue.severity == Severity::Error)
                        .map(|issue| issue.message.clone())
                        .collect();
                    assert!(errors.is_empty(), "{profile}: {}", errors.join("; "));
                }
            })
            .await,
        );
    }
    record(
        "ncp-patient-minimal",
        "The PDQm answers for the shared population, validated against MyHealth@EU NCP API 9.1.1-ci Patient-minimal and PDQm 3.2.0 Patient",
        cases,
    );
}
