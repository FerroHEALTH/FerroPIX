// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Writes the console's `Msg` variants from the English catalog, and
//! rebuilds the console when a catalog file is added to or removed from
//! `locales/`: the catalog's loader embeds every file it finds there, and
//! the compiler tracks only the files it embedded.
//!
//! Each message of `locales/en/ferropix-console.ftl` becomes one variant,
//! named by its id in upper camel case and documented by the `#` comment
//! right above it. The marks `number-group` and `number-decimal` are the
//! language's number format, never a message the pages draw, so they get
//! no variant. No specification governs the catalog layout: our own design.

use std::error::Error;
use std::fmt::Write as _;
use std::path::PathBuf;

/// The reference catalog, relative to the manifest.
const CATALOG: &str = "locales/en/ferropix-console.ftl";

/// The ids of the marks that are no message.
const MARKS: &[&str] = &["number-group", "number-decimal"];

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed=locales");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let catalog = std::fs::read_to_string(manifest.join(CATALOG))?;
    let out = PathBuf::from(std::env::var("OUT_DIR")?).join("messages.rs");
    std::fs::write(out, variants(&catalog)?)?;
    Ok(())
}

/// The `messages!` invocation for every message of `catalog`, in catalog
/// order.
fn variants(catalog: &str) -> Result<String, Box<dyn Error>> {
    let mut out = String::from("messages! {\n");
    let mut comment: Vec<&str> = Vec::new();
    for line in catalog.lines() {
        if let Some(text) = line.strip_prefix("# ") {
            comment.push(text);
            continue;
        }
        if line == "#" {
            comment.push("");
            continue;
        }
        if let Some(id) = message_id(line).filter(|id| !MARKS.contains(id)) {
            if comment.is_empty() {
                return Err(format!("{CATALOG}: the message {id} has no comment").into());
            }
            for text in &comment {
                writeln!(out, "    #[doc = {:?}]", format!(" {text}"))?;
            }
            writeln!(out, "    {} = {id:?},", name(id))?;
        }
        comment.clear();
    }
    out.push_str("}\n");
    Ok(out)
}

/// The id of the message `line` opens, when it opens one.
fn message_id(line: &str) -> Option<&str> {
    line.split_once(" =").map(|(id, _)| id).filter(|id| {
        id.starts_with(|c: char| c.is_ascii_lowercase())
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    })
}

/// The variant name of the message `id`: its words in upper camel case.
fn name(id: &str) -> String {
    id.split('-')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect()
}
