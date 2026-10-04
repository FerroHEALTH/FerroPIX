// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The feature boundaries, read from the crate graph one feature at a time
//! (no specification governs them: our own design).
//!
//! Only `xcpd` carries the SOAP 1.2, HL7 v3 and XUA stack: a build of any
//! other feature compiles none of the crates `xcpd` adds and names none of
//! them as a dependency of its own. `quick-xml` itself is in every FHIR
//! feature's graph already, through `fhir-types`, which depends on it
//! unconditionally; what `xcpd` adds is the crate's own use of it, its
//! message and query ids, and its clock. `atna` writes XML and timestamps
//! too, so it shares the clock and the XML writer, and never the message ids.
//! No feature reaches a crate of the application this crate was written for,
//! so any caller can use it as it is.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// Every feature of the crate.
const FEATURES: &[&str] = &["atna", "balp", "pixm", "pdqm", "mcsd", "pmir", "xcpd"];

/// The crates only `xcpd` compiles.
const XCPD_ONLY: &[&str] = &["jiff", "uuid"];

/// The crates `xcpd` names as dependencies of its own for its stack.
const XCPD_DIRECT: &[&str] = &["jiff", "quick-xml", "uuid"];

/// The workspace root, two levels above this crate's manifest.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The package names `cargo tree` lists for this crate with only `feature`
/// on, through normal dependencies, down to `depth` when given.
fn tree(feature: &str, depth: Option<&str>) -> BTreeSet<String> {
    let mut command = Command::new(env!("CARGO"));
    command
        .arg("tree")
        .arg("--manifest-path")
        .arg(Path::new(ROOT).join("Cargo.toml"))
        .args(["--package", "ihe-iti"])
        .args(["--edges", "normal"])
        .args(["--prefix", "none", "--format", "{p}"])
        .args(["--no-default-features", "--features", feature, "--locked"]);
    if let Some(depth) = depth {
        command.args(["--depth", depth]);
    }
    let output = command.output().expect("cargo tree runs");
    assert!(
        output.status.success(),
        "cargo tree failed for {feature}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let names: BTreeSet<String> = String::from_utf8(output.stdout)
        .expect("UTF-8 output")
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect();
    assert!(
        names.contains("ihe-iti"),
        "cargo tree for {feature} did not list the crate itself, so its output was not read"
    );
    names
}

/// The package names of the members under one workspace directory.
fn members(directory: &str) -> BTreeSet<String> {
    std::fs::read_dir(Path::new(ROOT).join(directory))
        .expect("a workspace directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("Cargo.toml").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn only_xcpd_compiles_the_soap_and_xua_stack() {
    let xcpd = tree("xcpd", None);
    for name in XCPD_ONLY {
        assert!(xcpd.contains(*name), "xcpd lost {name}: {xcpd:?}");
    }
    let xcpd_direct = tree("xcpd", Some("1"));
    for name in XCPD_DIRECT {
        assert!(xcpd_direct.contains(*name), "xcpd names {name}");
    }
    for feature in ["atna", "balp"] {
        let closure = tree(feature, None);
        assert!(
            !closure.contains("uuid"),
            "{feature} compiles the message ids: {closure:?}"
        );
    }
    for feature in FEATURES
        .iter()
        .filter(|feature| !["xcpd", "atna", "balp"].contains(*feature))
    {
        let closure = tree(feature, None);
        let leaked: Vec<&&str> = XCPD_ONLY
            .iter()
            .filter(|name| closure.contains(**name))
            .collect();
        assert!(leaked.is_empty(), "{feature} compiles {leaked:?}");
        let direct = tree(feature, Some("1"));
        let named: Vec<&&str> = XCPD_DIRECT
            .iter()
            .filter(|name| direct.contains(**name))
            .collect();
        assert!(named.is_empty(), "{feature} names {named:?} itself");
    }
}

#[test]
fn no_feature_depends_on_anything_in_ferrofed() {
    let mut internal = members("crates");
    internal.extend(members("app"));
    internal.extend(members("tools"));
    assert!(internal.remove("ihe-iti"), "the crate map lost ihe-iti");
    assert!(
        internal.contains("ferrofed-identity"),
        "the crate map lost the app"
    );
    for feature in FEATURES {
        let closure = tree(feature, None);
        let reached: Vec<&String> = internal
            .iter()
            .filter(|name| closure.contains(*name))
            .collect();
        assert!(reached.is_empty(), "{feature} depends on {reached:?}");
    }
}
