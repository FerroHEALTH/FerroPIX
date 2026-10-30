// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's crate boundary, read from the graph Cargo resolves: the
//! console links no workspace crate beside `ferropix-operator-api`, the
//! operator API's contract (`docs/architecture.md` §12.1), so the console
//! never runs the index itself; and it links no crate of the Leptos
//! framework on any target, since every page is drawn on the server (FerroUI
//! `docs/migration-0.6.md`).
//!
//! The walk follows normal and build edges from the console through the
//! whole resolved graph, so a workspace crate reached through a third-party
//! crate counts as well; dev-dependencies link into no shipped artifact and
//! are not followed. The tests that inject an edge into the real graph show
//! the check refuses what it exists to refuse.
#![expect(
    clippy::disallowed_types,
    reason = "the wire seam of `cargo metadata`, whose resolved graph is read as JSON"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde_json::{Value, json};

/// The console's package name.
const CONSOLE: &str = "ferropix-console";

/// The workspace crates the console may link.
const SERVER_ALLOWED: [&str; 1] = ["ferropix-operator-api"];

/// The crates of a hydrated console beside those named `leptos…`: the
/// Leptos framework's server functions and executor, and the panic hook of
/// its browser half, none of which a server-rendered console links.
const HYDRATED: [&str; 3] = ["server_fn", "any_spawner", "console_error_panic_hook"];

/// The target a hydrated console's browser half compiled for.
const BROWSER: &str = "wasm32-unknown-unknown";

/// The resolved graph `cargo metadata` answers for the workspace, every
/// target included or only `platform`'s.
fn metadata(platform: Option<&str>) -> Value {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let mut command = Command::new(cargo);
    command.args([
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--manifest-path",
        manifest,
    ]);
    if let Some(platform) = platform {
        command.args(["--filter-platform", platform]);
    }
    let output = command.output().expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cargo metadata answers JSON")
}

/// The package id of `name` among the workspace members.
fn member_id(metadata: &Value, name: &str) -> String {
    let members: BTreeSet<&str> = metadata["workspace_members"]
        .as_array()
        .expect("a member list")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    metadata["packages"]
        .as_array()
        .expect("a package list")
        .iter()
        .find(|package| {
            package["name"] == name && members.contains(package["id"].as_str().unwrap_or_default())
        })
        .and_then(|package| package["id"].as_str())
        .unwrap_or_else(|| panic!("{name} is a workspace member"))
        .to_owned()
}

/// Every package `root` reaches through normal and build edges, by name,
/// split into workspace members and the rest.
fn closure(metadata: &Value, root: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let names: BTreeMap<&str, &str> = metadata["packages"]
        .as_array()
        .expect("a package list")
        .iter()
        .filter_map(|package| Some((package["id"].as_str()?, package["name"].as_str()?)))
        .collect();
    let members: BTreeSet<&str> = metadata["workspace_members"]
        .as_array()
        .expect("a member list")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let nodes: BTreeMap<&str, &Value> = metadata["resolve"]["nodes"]
        .as_array()
        .expect("a resolved graph")
        .iter()
        .filter_map(|node| Some((node["id"].as_str()?, node)))
        .collect();
    let root = member_id(metadata, root);
    let mut seen = BTreeSet::from([root.clone()]);
    let mut queue = vec![root.clone()];
    while let Some(id) = queue.pop() {
        let node = nodes.get(id.as_str()).expect("every reached id is a node");
        for dep in node["deps"].as_array().expect("a dependency list") {
            let linked = dep["dep_kinds"]
                .as_array()
                .expect("dependency kinds")
                .iter()
                .any(|kind| kind["kind"] != "dev");
            let pkg = dep["pkg"].as_str().expect("a package id");
            if linked && seen.insert(pkg.to_owned()) {
                queue.push(pkg.to_owned());
            }
        }
    }
    seen.remove(&root);
    let (workspace, other): (Vec<&String>, Vec<&String>) =
        seen.iter().partition(|id| members.contains(id.as_str()));
    let name = |id: &String| names[id.as_str()].to_owned();
    (
        workspace.into_iter().map(name).collect(),
        other.into_iter().map(name).collect(),
    )
}

/// The workspace crates in `reached` outside `allowed`.
fn outside(reached: &BTreeSet<String>, allowed: &[&str]) -> BTreeSet<String> {
    reached
        .iter()
        .filter(|name| !allowed.contains(&name.as_str()))
        .cloned()
        .collect()
}

/// The crates of a hydrated console in `reached`.
fn hydrated(reached: &BTreeSet<String>) -> BTreeSet<String> {
    reached
        .iter()
        .filter(|name| HYDRATED.contains(&name.as_str()) || name.starts_with("leptos"))
        .cloned()
        .collect()
}

/// Adds the package `name`, no workspace member, to `metadata` with an
/// edge to it from the workspace member `from`.
fn inject_package(metadata: &mut Value, from: &str, name: &str) {
    let id = format!("injected+{name}");
    metadata["packages"]
        .as_array_mut()
        .expect("a package list")
        .push(json!({ "id": id, "name": name }));
    metadata["resolve"]["nodes"]
        .as_array_mut()
        .expect("a resolved graph")
        .push(json!({ "id": id, "deps": [] }));
    let from = member_id(metadata, from);
    metadata["resolve"]["nodes"]
        .as_array_mut()
        .expect("a resolved graph")
        .iter_mut()
        .find(|node| node["id"] == from.as_str())
        .expect("the node of the edge's source")["deps"]
        .as_array_mut()
        .expect("a dependency list")
        .push(json!({
            "name": name,
            "pkg": id,
            "dep_kinds": [{ "kind": null, "target": null }],
        }));
}

/// Adds an edge of `kind` (`null` for a normal one) from `from` to `to`,
/// both workspace members, to `metadata`'s resolved graph.
fn inject(metadata: &mut Value, from: &str, to: &str, kind: &Value) {
    let from = member_id(metadata, from);
    let to = member_id(metadata, to);
    let node = metadata["resolve"]["nodes"]
        .as_array_mut()
        .expect("a resolved graph")
        .iter_mut()
        .find(|node| node["id"] == from.as_str())
        .expect("the node of the edge's source");
    node["deps"]
        .as_array_mut()
        .expect("a dependency list")
        .push(json!({
            "name": "injected",
            "pkg": to,
            "dep_kinds": [{ "kind": kind.clone(), "target": null }],
        }));
}

#[test]
fn the_console_links_no_workspace_crate_outside_the_boundary() {
    let metadata = metadata(None);
    let (workspace, other) = closure(&metadata, CONSOLE);
    assert_eq!(
        BTreeSet::new(),
        outside(&workspace, &SERVER_ALLOWED),
        "the console links a workspace crate the boundary does not allow"
    );
    assert!(
        other.contains("ferrohealth-ui") && other.contains("axum"),
        "the walk reaches the console's own dependencies: {other:?}"
    );
}

#[test]
fn the_console_links_no_crate_of_the_leptos_framework_on_any_target() {
    for platform in [None, Some(BROWSER)] {
        let metadata = metadata(platform);
        let (workspace, other) = closure(&metadata, CONSOLE);
        assert_eq!(
            BTreeSet::new(),
            hydrated(&other),
            "the console links a crate of a hydrated console ({platform:?})"
        );
        assert_eq!(
            BTreeSet::new(),
            outside(&workspace, &SERVER_ALLOWED),
            "the console links a workspace crate the boundary does not allow ({platform:?})"
        );
    }
}

#[test]
fn an_injected_crate_of_the_leptos_framework_is_refused() {
    for name in ["leptos", "leptos_reactive", "server_fn"] {
        let mut metadata = metadata(None);
        inject_package(&mut metadata, CONSOLE, name);
        let (_, other) = closure(&metadata, CONSOLE);
        assert!(
            hydrated(&other).contains(name),
            "the check names the injected {name}: {other:?}"
        );
    }
}

#[test]
fn an_injected_direct_dependency_on_another_workspace_crate_is_refused() {
    let mut metadata = metadata(None);
    inject(&mut metadata, CONSOLE, "ihe-iti", &Value::Null);
    let (workspace, _) = closure(&metadata, CONSOLE);
    assert!(
        outside(&workspace, &SERVER_ALLOWED).contains("ihe-iti"),
        "the check names the injected crate: {workspace:?}"
    );
}

#[test]
fn an_injected_dependency_reached_through_another_crate_is_refused() {
    let mut metadata = metadata(None);
    inject(
        &mut metadata,
        CONSOLE,
        "nl-generic-functions",
        &json!("build"),
    );
    inject(
        &mut metadata,
        "nl-generic-functions",
        "ihe-iti",
        &Value::Null,
    );
    let (workspace, _) = closure(&metadata, CONSOLE);
    assert!(
        outside(&workspace, &SERVER_ALLOWED).contains("ihe-iti"),
        "the check follows the graph past the first crate: {workspace:?}"
    );
}

#[test]
fn an_injected_dependency_on_the_browser_target_is_refused() {
    let mut metadata = metadata(Some(BROWSER));
    inject(&mut metadata, CONSOLE, "ihe-iti", &Value::Null);
    let (workspace, _) = closure(&metadata, CONSOLE);
    assert!(
        outside(&workspace, &SERVER_ALLOWED).contains("ihe-iti"),
        "the check names the injected crate on that target: {workspace:?}"
    );
}

#[test]
fn a_dev_dependency_links_into_nothing_and_is_not_followed() {
    let mut metadata = metadata(None);
    inject(&mut metadata, CONSOLE, "ihe-iti", &json!("dev"));
    let (workspace, _) = closure(&metadata, CONSOLE);
    assert!(
        !workspace.contains("ihe-iti"),
        "a dev-dependency is outside the shipped closure: {workspace:?}"
    );
}
