// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

use ferropix_testkit::fixtures::{FIXTURE_ROOT, Fixture};
use ferropix_testkit::guard::{check_tree, json_files};
use fhir_types::r4::resource::Resource;
use std::path::{Path, PathBuf};

#[test]
fn every_fixture_decodes_into_its_fhir_r4_resource() {
    for fixture in Fixture::ALL {
        let resource = fixture
            .resource()
            .unwrap_or_else(|error| panic!("{fixture:?}: {error:?}"));
        let decoded = match resource {
            Resource::Patient(_) => "Patient",
            Resource::Parameters(_) => "Parameters",
            Resource::Bundle(_) => "Bundle",
            _ => "another resource",
        };
        assert_eq!(fixture.resource_type(), decoded, "{fixture:?}");
    }
}

#[test]
fn every_file_under_the_fixture_tree_is_a_listed_fixture() {
    let listed: Vec<PathBuf> = Fixture::ALL.iter().map(|fixture| fixture.path()).collect();
    for file in json_files(Path::new(FIXTURE_ROOT)).unwrap() {
        assert!(
            listed.contains(&file),
            "{} is not a variant of Fixture",
            file.display()
        );
    }
}

#[test]
fn every_committed_fixture_passes_the_guard() {
    let found = check_tree(Path::new(FIXTURE_ROOT)).unwrap();
    assert!(
        found.is_empty(),
        "fixtures holding non-synthetic values: {found:?}"
    );
}

#[test]
fn every_transaction_has_a_fixture() {
    for transaction in ["ITI-104", "ITI-83", "ITI-78", "ITI-119", "ITI-93"] {
        assert!(
            Fixture::ALL
                .iter()
                .any(|fixture| fixture.transaction() == transaction),
            "no fixture for {transaction}"
        );
    }
}

#[test]
fn the_pix_answer_is_a_parameters_of_target_identifiers() {
    let Resource::Parameters(parameters) = Fixture::PixQueryOut.resource().unwrap() else {
        panic!("the ITI-83 answer is a Parameters");
    };
    let text = format!("{parameters:?}");
    assert!(text.contains("targetIdentifier"), "{text}");
    assert!(text.contains("targetId"), "{text}");
}
