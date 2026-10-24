// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

use ferropix_testkit::guard::{Violation, check, check_tree};
use ferropix_testkit::synthetic::{bsn, is_eleven_proof};
use fhir_types::codec::Value;
use nl_generic_functions::identification::BSN_SYSTEMS;
use std::fs;
use std::path::PathBuf;

const BSN_SYSTEM: &str = "http://fhir.nl/fhir/NamingSystem/bsn";

fn document(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// The lowest nine-digit number that passes the eleven-proof outside the
/// synthetic range, computed so the tree holds no literal one.
fn valid_bsn_outside_the_range() -> String {
    (100_000_000_u32..=100_000_099)
        .map(|number| number.to_string())
        .find(|digits| is_eleven_proof(digits))
        .unwrap()
}

fn patient_with_bsn(system: &str, value: &str) -> Value {
    document(&format!(
        r#"{{"resourceType":"Patient","identifier":[{{"system":"{system}","value":"{value}"}}]}}"#
    ))
}

fn name_refused(path: &str) -> Vec<Violation> {
    vec![Violation::NameOutsideSet {
        path: path.to_owned(),
    }]
}

#[test]
fn a_synthetic_bsn_passes() {
    let value = bsn(3).unwrap().digits();
    assert_eq!(
        Vec::<Violation>::new(),
        check(&patient_with_bsn(BSN_SYSTEM, &value))
    );
}

#[test]
fn a_bsn_outside_the_synthetic_range_is_refused() {
    let value = valid_bsn_outside_the_range();
    assert_eq!(
        vec![Violation::BsnOutsideRange {
            path: "$.identifier[0].value".to_owned()
        }],
        check(&patient_with_bsn(BSN_SYSTEM, &value))
    );
}

#[test]
fn a_bsn_in_the_range_that_fails_the_proof_is_refused() {
    let mut value = bsn(0).unwrap().digits();
    let last = if value.ends_with('9') { '0' } else { '9' };
    value.pop();
    value.push(last);
    assert!(
        !is_eleven_proof(&value),
        "the changed number fails the proof"
    );
    assert_eq!(
        vec![Violation::BsnOutsideRange {
            path: "$.identifier[0].value".to_owned()
        }],
        check(&patient_with_bsn(BSN_SYSTEM, &value))
    );
}

#[test]
fn every_bsn_naming_system_is_held_to_the_range() {
    let value = valid_bsn_outside_the_range();
    for system in BSN_SYSTEMS {
        assert_eq!(
            1,
            check(&patient_with_bsn(system, &value)).len(),
            "{system} is a BSN system"
        );
    }
}

#[test]
fn a_bsn_in_a_source_identifier_token_is_refused() {
    let value = valid_bsn_outside_the_range();
    let parameters = document(&format!(
        r#"{{"resourceType":"Parameters","parameter":[{{"name":"sourceIdentifier","valueString":"{BSN_SYSTEM}|{value}"}}]}}"#
    ));
    assert_eq!(
        vec![Violation::BsnOutsideRange {
            path: "$.parameter[0].valueString".to_owned()
        }],
        check(&parameters)
    );
}

#[test]
fn a_bsn_under_another_name_is_refused() {
    let value = valid_bsn_outside_the_range();
    let patient = document(&format!(
        r#"{{"resourceType":"Patient","telecom":[{{"system":"phone","value":"{value}"}}]}}"#
    ));
    assert_eq!(
        vec![Violation::UndeclaredBsn {
            path: "$.telecom[0].value".to_owned()
        }],
        check(&patient)
    );
}

#[test]
fn a_family_name_outside_the_set_is_refused() {
    let patient =
        document(r#"{"resourceType":"Patient","name":[{"family":"Somebody","given":["Ada"]}]}"#);
    assert_eq!(name_refused("$.name[0].family"), check(&patient));
}

#[test]
fn a_given_name_outside_the_set_is_refused() {
    let patient = document(
        r#"{"resourceType":"Patient","name":[{"family":"Proefsma","given":["Ada","Someone"]}]}"#,
    );
    assert_eq!(name_refused("$.name[0].given[1]"), check(&patient));
}

#[test]
fn a_name_text_outside_the_set_is_refused() {
    let patient = document(r#"{"resourceType":"Patient","name":[{"text":"Ada Somebody"}]}"#);
    assert_eq!(name_refused("$.name[0].text"), check(&patient));
}

#[test]
fn a_name_inside_a_bundle_entry_is_checked() {
    let bundle = document(
        r#"{"resourceType":"Bundle","type":"searchset","entry":[{"resource":{"resourceType":"Patient","name":[{"family":"Somebody"}]}}]}"#,
    );
    assert_eq!(
        name_refused("$.entry[0].resource.name[0].family"),
        check(&bundle)
    );
}

#[test]
fn the_tree_check_names_the_offending_file() {
    let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("guard-tree-{}", std::process::id()));
    let good = scratch.join("good.json");
    let nested = scratch.join("nested");
    let bad = nested.join("bad.json");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        &good,
        r#"{"resourceType":"Patient","name":[{"family":"Proefsma"}]}"#,
    )
    .unwrap();
    fs::write(
        &bad,
        r#"{"resourceType":"Patient","name":[{"family":"Somebody"}]}"#,
    )
    .unwrap();
    let found = check_tree(&scratch).unwrap();
    fs::remove_file(&good).unwrap();
    fs::remove_file(&bad).unwrap();
    assert_eq!(1, found.len(), "{found:?}");
    assert_eq!(bad, found[0].0, "the bad file is named");
}
