// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The book's configuration page lists every key of the tree, every `toml`
//! block of the book is a configuration the server's loader reads, and its
//! problem types page has a section for every problem type.
//!
//! The keys are read from the tree itself: a production and a development
//! configuration with every section set are written back, so a key added to
//! a section and missing from the page fails here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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
cache_ttl = "1min"

[[auth.basic.users]]
user = "dev-steward"
password_hash = "{PHC}"
scopes = ["__ferropix.steward"]

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
        Path::new(env!("CARGO_MANIFEST_DIR"))
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
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../website/book/src/{relative}.md")),
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

/// Returns every markdown file under `directory`, in path order.
fn pages(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)
        .expect("the book's directory is read")
        .map(|entry| entry.expect("a directory entry is read").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            found.extend(pages(&path));
        } else if path.extension().is_some_and(|extension| extension == "md") {
            found.push(path);
        }
    }
    found
}

/// Returns each fenced block of `page` whose info string is exactly `info`,
/// with the line its fence opens on.
fn blocks(page: &str, info: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (number, line) in page.lines().enumerate() {
        match open.take() {
            Some((start, body)) if line.trim_end() == "```" => found.push((start, body)),
            Some((start, mut body)) => {
                body.push_str(line);
                body.push('\n');
                open = Some((start, body));
            }
            None if line
                .strip_prefix("```")
                .is_some_and(|rest| rest.trim_end() == info) =>
            {
                open = Some((number + 1, String::new()));
            }
            None => {}
        }
    }
    found
}

#[test]
fn every_toml_block_of_the_book_is_a_configuration_the_server_reads() {
    let book = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../website/book/src");
    let mut read = 0;
    let mut refused = Vec::new();
    for page in pages(&book) {
        let text = std::fs::read_to_string(&page).expect("a page of the book is read");
        for (line, block) in blocks(&text, "toml") {
            read += 1;
            if let Err(error) = Config::from_sources(Some(&block), &BTreeMap::new()) {
                refused.push(format!("{}:{line}: {error}", page.display()));
            }
        }
    }
    assert!(
        read >= 5,
        "the book holds {read} toml blocks; the walk found too few"
    );
    assert!(
        refused.is_empty(),
        "the server's loader refuses these blocks of the book:\n{}",
        refused.join("\n")
    );
}

#[test]
fn a_block_with_a_key_the_server_does_not_know_is_refused() {
    let page = "Text.\n\n```toml\n[server]\nlisten = \"0.0.0.0:8080\"\nlisten_port = 8080\n```\n\n```toml,ferroterm\n[terminology]\n```\n";
    let found = blocks(page, "toml");
    assert_eq!(1, found.len(), "{found:?}");
    let (line, block) = &found[0];
    assert_eq!(3, *line);
    assert!(Config::from_sources(Some(block), &BTreeMap::new()).is_err());
}
