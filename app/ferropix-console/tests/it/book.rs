// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Every `toml,console` block of the book is a configuration the console's
//! loader reads, so a key the book shows is a key the console takes.

use std::collections::BTreeMap;
use std::path::Path;

use ferropix_console::config::Config;

/// Returns the body of each fenced block of `page` whose info string is
/// `toml,console`, with the line its fence opens on.
fn console_blocks(page: &str) -> Vec<(usize, String)> {
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
            None if line.trim_end() == "```toml,console" => {
                open = Some((number + 1, String::new()));
            }
            None => {}
        }
    }
    found
}

#[test]
fn every_console_block_of_the_book_is_a_configuration_the_console_reads() {
    let page =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../website/book/src/operate/console.md");
    let text = std::fs::read_to_string(&page).expect("the console page is read");
    let found = console_blocks(&text);
    assert!(
        !found.is_empty(),
        "the console page holds no toml,console block"
    );
    let refused: Vec<String> = found
        .iter()
        .filter_map(|(line, block)| {
            Config::from_sources(Some(block), &BTreeMap::new())
                .err()
                .map(|error| format!("console.md:{line}: {error}"))
        })
        .collect();
    assert!(
        refused.is_empty(),
        "the console's loader refuses these blocks of the book:\n{}",
        refused.join("\n")
    );
}

#[test]
fn a_console_block_with_a_key_the_console_does_not_know_is_refused() {
    let found =
        console_blocks("```toml,console\n[pix]\nbase_uri = \"https://pix.example.org/\"\n```\n");
    assert_eq!(1, found.len());
    assert!(Config::from_sources(Some(&found[0].1), &BTreeMap::new()).is_err());
}
