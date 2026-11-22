// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The records an audit spool holds on disk, as the suites read them.
//!
//! `ihe-atna` 0.24.2 appends the records of each batch to the active segment
//! `{first:020}.open` and seals a full one as `{first:020}.seg`, each record
//! framed by its length and its CRC-32, four bytes big-endian each, with a
//! sidecar `{first:020}.sent` counting the records settled from the first
//! (FerroHEALTH/FerroAUDIT#254, #264). A suite runs no repository, so it reads
//! every record of every segment, sealed or active, after the settled ones,
//! in sequence order. No specification governs the layout: `ihe-atna`'s own
//! design.

use std::path::{Path, PathBuf};

/// The records of every segment in `directory`, in sequence order; none
/// when the directory does not exist.
#[must_use]
pub fn records(directory: &Path) -> Vec<Vec<u8>> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut segments: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "seg" || extension == "open")
        })
        .collect();
    segments.sort();
    let mut records = Vec::new();
    for segment in segments {
        let Ok(bytes) = std::fs::read(&segment) else {
            continue;
        };
        let settled = std::fs::read_to_string(segment.with_extension("sent"))
            .ok()
            .and_then(|text| text.trim().parse::<usize>().ok())
            .unwrap_or(0);
        records.extend(frames(&bytes).into_iter().skip(settled));
    }
    records
}

/// The records framed in a segment's `bytes`, up to the first that is not
/// whole.
fn frames(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut at = 0_usize;
    while let Some(header) = at.checked_add(8).and_then(|end| bytes.get(at..end)) {
        let Some(len) = header
            .get(..4)
            .and_then(|len| <[u8; 4]>::try_from(len).ok())
            .and_then(|len| usize::try_from(u32::from_be_bytes(len)).ok())
        else {
            break;
        };
        let start = at.saturating_add(8);
        let Some(record) = start.checked_add(len).and_then(|end| bytes.get(start..end)) else {
            break;
        };
        frames.push(record.to_vec());
        at = start.saturating_add(len);
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::frames;

    #[test]
    fn the_whole_frames_read_in_order() {
        let mut bytes = Vec::new();
        for record in [b"one".as_slice(), b"three"] {
            bytes.extend_from_slice(&u32::try_from(record.len()).unwrap().to_be_bytes());
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            bytes.extend_from_slice(record);
        }
        bytes.extend_from_slice(&[0, 0, 0, 9, 0]);
        assert_eq!(
            vec![b"one".to_vec(), b"three".to_vec()],
            frames(&bytes),
            "the torn tail is not read"
        );
    }
}
