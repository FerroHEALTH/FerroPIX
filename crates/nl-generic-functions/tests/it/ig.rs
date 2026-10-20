// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The vendored IG source (`docs/specs/nl-gf/`, package `fhir.nl.gf`
//! 0.3.0), read by the tests that hold the clients to it.

use std::path::Path;

/// The vendored IG source root, three levels above this crate's manifest.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/specs/nl-gf");

/// The text of the vendored file at `path` under the IG source root.
pub(crate) fn source(path: &str) -> String {
    std::fs::read_to_string(Path::new(ROOT).join(path))
        .unwrap_or_else(|error| panic!("the vendored IG file {path} reads: {error}"))
}

/// Whether the FSH text `fsh` holds the rule line `rule`, ignoring the
/// indentation FSH nests rules with.
pub(crate) fn has_rule(fsh: &str, rule: &str) -> bool {
    fsh.lines().any(|line| line.trim() == rule)
}

#[test]
fn the_vendored_source_is_the_pinned_package_version() {
    let config = source("sushi-config.yaml");
    assert!(has_rule(&config, "id: fhir.nl.gf"), "the package id");
    assert!(
        config
            .lines()
            .any(|line| line.trim_start().starts_with("version: 0.3.0")),
        "the version Annex B names"
    );
}

#[test]
fn the_identifier_systems_are_the_igs() {
    let aliases = source("input/fsh/aliases.fsh");
    assert!(has_rule(
        &aliases,
        &format!(
            "Alias: $ura = {}",
            nl_generic_functions::identification::URA_SYSTEM
        )
    ));
    let naming = source("input/fsh/namingsystem.fsh");
    assert!(
        naming.contains(&format!(
            "uniqueId[=].value = \"{}\"",
            nl_generic_functions::identification::PSEUDO_BSN_SYSTEM
        )),
        "the pseudo-bsn NamingSystem"
    );
}
