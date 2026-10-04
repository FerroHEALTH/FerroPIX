// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The URA of an NL-GF `Organization`, read from organisations shaped after
//! the IG's LRZa and Query Directory examples
//! (`input/fsh/examples/admin-directory-lrza.fsh`,
//! `input/fsh/examples/query-directory.fsh`), with synthetic URAs only.
#![expect(
    clippy::disallowed_types,
    reason = "the test seam: the fixtures build FHIR JSON as values"
)]

use fhir_types::codec::{Json, Path};
use fhir_types::r4::organization::Organization;
use nl_generic_functions::identification::Ura;
use nl_generic_functions::lrza::{self, LrzaError};
use serde_json::{Value, json};

use crate::ig::{has_rule, source};

/// The author-assigned URA identifier of the LRZa example, with a synthetic
/// URA and a synthetic assigner.
fn ura_identifier(value: &str) -> Value {
    json!({
        "system": "http://fhir.nl/fhir/NamingSystem/ura",
        "value": value,
        "assigner": {
            "identifier": {
                "system": "http://fhir.nl/fhir/NamingSystem/kvk",
                "value": "kvk-test-0001",
                "type": {
                    "coding": [{
                        "system": "http://terminology.hl7.org/CodeSystem/provenance-participant-type",
                        "code": "author"
                    }]
                }
            }
        }
    })
}

fn organization(body: &Value) -> Organization {
    let value: fhir_types::codec::Value = serde_json::from_value(body.clone()).expect("FHIR JSON");
    let object = value.as_object().expect("a resource object");
    Organization::from_json(object, &mut Path::root("Organization")).expect("an Organization")
}

#[test]
fn the_lrza_example_identifies_its_organisations_by_ura() {
    let example = source("input/fsh/examples/admin-directory-lrza.fsh");
    assert!(
        example.lines().any(|line| line.trim().starts_with(
            "* insert AuthorAssignedIdentifier(\"http://fhir.nl/fhir/NamingSystem/ura\""
        )),
        "the LRZa example's Organizations carry an author-assigned URA identifier"
    );
    let rules = source("input/fsh/structuredefinitions.fsh");
    assert!(
        rules.contains(
            "identifier.where(system='http://fhir.nl/fhir/NamingSystem/ura').exists() or partOf.exists()"
        ),
        "the ura-identifier-or-partof invariant"
    );
    assert!(has_rule(&rules, "* obeys ura-identifier-or-partof"));
}

#[test]
fn a_top_level_organisation_names_its_ura() {
    let top = organization(&json!({
        "resourceType": "Organization",
        "id": "lrza-o1",
        "identifier": [ura_identifier("ura-test-0001")],
        "name": "Synthetic General Practice",
        "endpoint": [{"reference": "Endpoint/lrza-e1"}]
    }));
    let ura = lrza::ura(&top).expect("a URA");
    assert_eq!(ura.as_ref().map(Ura::as_str), Some("ura-test-0001"));
}

#[test]
fn a_department_is_part_of_its_care_provider_and_names_no_ura() {
    let department = organization(&json!({
        "resourceType": "Organization",
        "id": "ad3-o2",
        "identifier": [{"system": "urn:oid:2.999.7", "value": "dept-1"}],
        "name": "Synthetic Department",
        "partOf": {"reference": "Organization/ad3-o1"}
    }));
    assert_eq!(lrza::ura(&department), Ok(None));
}

#[test]
fn an_organisation_with_neither_breaks_the_invariant() {
    let neither = organization(&json!({
        "resourceType": "Organization",
        "id": "o-x",
        "identifier": [{"system": "urn:oid:2.999.7", "value": "x-1"}],
        "name": "Synthetic Unattached"
    }));
    assert_eq!(lrza::ura(&neither), Err(LrzaError::Missing));
}

#[test]
fn two_different_uras_name_no_one_care_provider() {
    let two = organization(&json!({
        "resourceType": "Organization",
        "id": "o-y",
        "identifier": [ura_identifier("ura-test-0001"), ura_identifier("ura-test-0002")],
        "name": "Synthetic Twice"
    }));
    assert_eq!(lrza::ura(&two), Err(LrzaError::Ambiguous));
    let repeated = organization(&json!({
        "resourceType": "Organization",
        "id": "o-z",
        "identifier": [ura_identifier("ura-test-0001"), ura_identifier("ura-test-0001")],
        "name": "Synthetic Repeated"
    }));
    assert_eq!(
        lrza::ura(&repeated)
            .expect("one URA")
            .map(|ura| ura.to_string()),
        Some("ura-test-0001".to_owned()),
        "the same URA twice is one care provider"
    );
}

#[test]
fn a_ura_identifier_without_a_value_is_refused() {
    let empty = organization(&json!({
        "resourceType": "Organization",
        "id": "o-e",
        "identifier": [{"system": "http://fhir.nl/fhir/NamingSystem/ura"}],
        "name": "Synthetic Empty"
    }));
    assert_eq!(lrza::ura(&empty), Err(LrzaError::EmptyValue));
}
