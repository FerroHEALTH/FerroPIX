// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The book's fenced code blocks, by info string, for the suites that hold
//! each configuration sample of `website/book/src` to the loader of the
//! program it configures.
//!
//! No specification governs the check: our own design (#594).

use std::path::{Path, PathBuf};

/// The book's sources, relative to this crate's manifest.
const BOOK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../website/book/src");

/// One fenced block of a page of the book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The page the block is on.
    pub page: PathBuf,
    /// The 1-based line its opening fence is on.
    pub line: usize,
    /// The lines between the fences, each ending in a line feed.
    pub body: String,
}

/// Returns every block of the book whose info string is exactly `info`, in
/// the order of the pages' paths and of the blocks on each page.
///
/// # Errors
///
/// Returns the I/O error of a directory or page that cannot be read.
pub fn blocks(info: &str) -> Result<Vec<Block>, std::io::Error> {
    let mut found = Vec::new();
    for page in pages(Path::new(BOOK))? {
        let text = std::fs::read_to_string(&page)?;
        found.extend(fenced(&text, info).into_iter().map(|(line, body)| Block {
            page: page.clone(),
            line,
            body,
        }));
    }
    Ok(found)
}

/// Returns each fenced block of `text` whose info string is exactly `info`,
/// with the 1-based line its opening fence is on.
#[must_use]
pub fn fenced(text: &str, info: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (number, line) in text.lines().enumerate() {
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

/// Returns every markdown file under `directory`, in path order.
fn pages(directory: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut entries = std::fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    let mut found = Vec::new();
    for path in entries {
        if path.is_dir() {
            found.extend(pages(&path)?);
        } else if path.extension().is_some_and(|extension| extension == "md") {
            found.push(path);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::{blocks, fenced};

    #[test]
    fn only_a_block_with_the_exact_info_string_is_taken() {
        let text = "Text.\n\n```toml\n[server]\nlisten = \"x\"\n```\n\n```toml,ferroterm\n[terminology]\n```\n";
        assert_eq!(
            vec![(3, String::from("[server]\nlisten = \"x\"\n"))],
            fenced(text, "toml")
        );
        assert_eq!(1, fenced(text, "toml,ferroterm").len());
        assert_eq!(0, fenced(text, "yaml").len());
    }

    #[test]
    fn the_book_is_walked() {
        assert_ne!(0, blocks("toml").expect("the book is read").len());
    }
}
