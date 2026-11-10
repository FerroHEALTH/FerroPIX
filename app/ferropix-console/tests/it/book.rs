// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Every `toml,console` block of the book is a configuration the console's
//! loader reads, so a key the book shows is a key the console takes.

use std::collections::BTreeMap;

use ferropix_console::config::Config;

#[test]
fn every_console_block_of_the_book_is_a_configuration_the_console_reads() {
    let found = ferropix_testkit::book::blocks("toml,console").expect("the book is read");
    assert!(!found.is_empty(), "the book holds no toml,console block");
    let refused: Vec<String> = found
        .iter()
        .filter_map(|block| {
            Config::from_sources(Some(&block.body), &BTreeMap::new())
                .err()
                .map(|error| format!("{}:{}: {error}", block.page.display(), block.line))
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
    let found = ferropix_testkit::book::fenced(
        "```toml,console\n[pix]\nbase_uri = \"https://pix.example.org/\"\n```\n",
        "toml,console",
    );
    assert_eq!(1, found.len());
    assert!(Config::from_sources(Some(&found[0].1), &BTreeMap::new()).is_err());
}
