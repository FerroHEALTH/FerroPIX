// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The book's configuration page lists every key of the tree, and its
//! problem types page has a section for every problem type.
//!
//! The keys are read from the tree itself: a production and a development
//! configuration with every section set are written back, so a key added to
//! a section and missing from the page fails here.

use std::collections::BTreeSet;

use ferropix_server::config::Config;

use crate::support::{FULL, PHC, Secrets};

/// A development configuration with the keys production refuses.
fn development() -> String {
    format!(
        r#"
profile = "development"

[db]
url = "postgres://db.invalid/pix"

[auth.basic]
cache_ttl_seconds = 60

[[auth.basic.users]]
user = "dev-source"
password_hash = "{PHC}"
scopes = ["system/Patient.cud"]

[dev]
loopback = true
"#
    )
}

/// The keys a written-back tree leaves out, because each excludes a key the
/// fixtures set.
const EXCLUSIVE: [&str; 1] = ["auth.basic.users[].password_hash_file"];

/// Adds every dotted key under `table` to `keys`, `prefix` naming the table.
fn walk(table: &toml::Table, prefix: &str, keys: &mut BTreeSet<String>) {
    for (name, value) in table {
        let key = if prefix == "matching.fields" {
            format!("{prefix}.<field>")
        } else if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            toml::Value::Table(inner) => walk(inner, &key, keys),
            toml::Value::Array(items) if items.is_empty() => {}
            toml::Value::Array(items) if items.iter().all(toml::Value::is_table) => {
                for item in items {
                    if let toml::Value::Table(inner) = item {
                        walk(inner, &format!("{key}[]"), keys);
                    }
                }
            }
            _ => {
                keys.insert(key);
            }
        }
    }
}

#[test]
fn the_configuration_page_lists_every_key_of_the_tree() {
    let secrets = Secrets::new();
    let mut keys: BTreeSet<String> = EXCLUSIVE.iter().map(|key| (*key).to_owned()).collect();
    for text in [FULL.to_owned(), development()] {
        let config =
            Config::from_sources(Some(&text), &secrets.environment()).expect("the fixture reads");
        let written: toml::Table =
            toml::from_str(&toml::to_string(&config).expect("the tree writes"))
                .expect("the written tree reads");
        walk(&written, "", &mut keys);
    }
    assert!(keys.contains("server.listen"), "{keys:?}");
    assert!(keys.contains("domain[].source_client_id"), "{keys:?}");
    assert!(keys.contains("matching.fields.<field>.m"), "{keys:?}");
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../website/book/src/operate/configuration.md"),
    )
    .expect("the configuration page is read");
    let missing: Vec<&String> = keys
        .iter()
        .filter(|key| !page.contains(&format!("`{key}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "the book's configuration page does not list {missing:?}"
    );
}

#[test]
fn every_problem_type_resolves_to_its_section_of_the_book() {
    let page_url = ferropix_server::problem::BASE
        .strip_suffix('#')
        .expect("the base ends in a fragment");
    let relative = page_url
        .strip_prefix("https://ferropix.eu/docs/")
        .and_then(|path| path.strip_suffix(".html"))
        .expect("the base is a page of the book");
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../website/book/src/{relative}.md")),
    )
    .expect("the problem types page is read");
    let types = ferropix_server::problem::types();
    let missing: Vec<&str> = types
        .iter()
        .filter_map(|kind| kind.slug())
        .filter(|slug| !page.contains(&format!("\n### `{slug}`\n")))
        .collect();
    assert!(
        missing.is_empty(),
        "problem types without a section on the page: {missing:?}"
    );
    for kind in types.iter() {
        assert!(
            kind.uri().starts_with(ferropix_server::problem::BASE),
            "{}",
            kind.uri()
        );
    }
}
