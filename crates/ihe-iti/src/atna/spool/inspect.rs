// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Checking that a spool would open in a directory, writing nothing, for a
//! configuration check that runs where the spool's volume is not mounted. No
//! specification governs it: our own design.

use std::fs;
use std::path::Path;

use super::disk::{check_private, first_foreign, io, writable_by_mode};
use super::{QUARANTINE, Spool, SpoolError};

impl Spool {
    /// Checks that a spool would open in `directory`, writing nothing.
    ///
    /// A directory that exists is held to what [`Spool::open_for`] holds it
    /// to, read in place: it and its quarantine give no group or other user
    /// access, hold no file the spool did not write, and the directory's mode
    /// lets its owner write in it. A partial file a crash left passes, as
    /// opening removes it. For a directory that does not exist, opening
    /// creates it and every missing directory above it, so the nearest
    /// ancestor that exists must be a directory whose mode lets someone write
    /// in it. Only modes are read, so a spool on a filesystem mounted
    /// read-only for the check passes, and whether the process user may write
    /// there is left to the open.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Exposed`] and [`SpoolError::Foreign`] as
    /// [`Spool::open_for`] returns them, [`SpoolError::NotADirectory`] for a
    /// path, or the nearest existing ancestor of a missing one, that is a
    /// file, [`SpoolError::Unwritable`] for a directory whose mode admits no
    /// writer, and [`SpoolError::Io`] when one cannot be read.
    pub fn inspect(directory: &Path) -> Result<(), SpoolError> {
        let metadata = match fs::metadata(directory) {
            Ok(metadata) => metadata,
            Err(error) if missing(&error) => return inspect_ancestor(directory),
            Err(source) => return Err(io("read", directory)(source)),
        };
        if !metadata.is_dir() {
            return Err(SpoolError::NotADirectory(directory.to_owned()));
        }
        check_private(directory)?;
        if !writable_by_mode(directory)? {
            return Err(SpoolError::Unwritable(directory.to_owned()));
        }
        let held = directory.join(QUARANTINE);
        match fs::metadata(&held) {
            Ok(metadata) if metadata.is_dir() => {
                check_private(&held)?;
                if let Some(path) = first_foreign(&held, None)? {
                    return Err(SpoolError::Foreign {
                        directory: held,
                        path,
                    });
                }
            }
            Ok(_) => return Err(SpoolError::NotADirectory(held)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(io("read", &held)(source)),
        }
        match first_foreign(directory, Some(&held))? {
            Some(path) => Err(SpoolError::Foreign {
                directory: directory.to_owned(),
                path,
            }),
            None => Ok(()),
        }
    }
}

/// Checks the nearest ancestor of `directory`, which does not exist, that
/// does: a directory whose mode lets someone write in it, as opening creates
/// every missing directory under it ([`Spool::inspect`]).
fn inspect_ancestor(directory: &Path) -> Result<(), SpoolError> {
    for ancestor in directory.ancestors().skip(1) {
        let ancestor = if ancestor.as_os_str().is_empty() {
            Path::new(".")
        } else {
            ancestor
        };
        match fs::metadata(ancestor) {
            Ok(metadata) if !metadata.is_dir() => {
                return Err(SpoolError::NotADirectory(ancestor.to_owned()));
            }
            Ok(_) if !writable_by_mode(ancestor)? => {
                return Err(SpoolError::Unwritable(ancestor.to_owned()));
            }
            Ok(_) => return Ok(()),
            Err(error) if missing(&error) => {}
            Err(source) => return Err(io("read", ancestor)(source)),
        }
    }
    Err(SpoolError::NotADirectory(directory.to_owned()))
}

/// Whether `error` says the path does not exist, or that a path above it is
/// a file, so the path is missing.
fn missing(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
    )
}
