// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pin matrix reader: a row of `docs/VERSIONS.md` by its item.
//!
//! A suite asserts a constant against the matrix through [`matrix_pin`], so
//! the image references of [`containers`](crate::containers) and the rows
//! the pin-freshness lane reads never disagree.

use std::path::PathBuf;

/// The pin matrix at `docs/VERSIONS.md`, relative to this crate's manifest.
const MATRIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/VERSIONS.md");

/// A pin could not be read from the matrix.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PinError {
    /// The matrix file could not be read.
    #[error("cannot read the pin matrix at {}", path.display())]
    Read {
        /// The path that was tried.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// No table row has the requested item in its first cell.
    #[error("the pin matrix has no row for {item}")]
    Missing {
        /// The item that was looked up.
        item: String,
    },
}

/// Returns the `Pin` cell of the matrix row whose first cell is `item`,
/// with its backticks removed and its surrounding space trimmed.
///
/// # Errors
///
/// Returns [`PinError::Read`] when `docs/VERSIONS.md` cannot be read and
/// [`PinError::Missing`] when no row carries `item`.
pub fn matrix_pin(item: &str) -> Result<String, PinError> {
    let path = PathBuf::from(MATRIX);
    let text = std::fs::read_to_string(&path).map_err(|source| PinError::Read {
        path: path.clone(),
        source,
    })?;
    row_pin(&text, item).ok_or_else(|| PinError::Missing {
        item: item.to_owned(),
    })
}

/// Returns the second cell of the first table row of `text` whose first
/// cell is `item`.
fn row_pin(text: &str, item: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut cells = line
            .strip_prefix('|')?
            .split('|')
            .map(|cell| cell.replace('`', "").trim().to_owned());
        let key = cells.next()?;
        let pin = cells.next()?;
        (key == item).then_some(pin)
    })
}

#[cfg(test)]
mod tests {
    use super::row_pin;

    #[test]
    fn a_row_is_found_by_its_first_cell() {
        let text = "| Item | Pin |\n|---|---|\n| Some image | `repo:1.0@sha256:00` |\n";
        assert_eq!(
            Some("repo:1.0@sha256:00".to_owned()),
            row_pin(text, "Some image")
        );
    }

    #[test]
    fn an_absent_item_has_no_pin() {
        assert_eq!(None, row_pin("| Item | Pin |\n", "Other"));
    }
}
