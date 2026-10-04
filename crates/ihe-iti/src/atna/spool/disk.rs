// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The files of a spool on disk: one per message, named by its sequence
//! number, written durably and readable by their owner alone. No
//! specification governs the layout: our own design.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use secrecy::{ExposeSecret, SecretSlice};

use super::{PARTIAL, STORED, SpoolError};

/// The messages in `directory`, by sequence number, with their sizes; a
/// partial write a crash left is removed, and `skip`, the quarantine, is
/// passed over.
pub(super) fn scan(
    directory: &Path,
    skip: Option<&Path>,
) -> Result<BTreeMap<u64, u64>, SpoolError> {
    let mut sizes = BTreeMap::new();
    for entry in fs::read_dir(directory).map_err(io("read", directory))? {
        let entry = entry.map_err(io("read", directory))?;
        let path = entry.path();
        if Some(path.as_path()) == skip {
            continue;
        }
        let extension = path.extension().and_then(OsStr::to_str);
        // NOTE: no specification governs this: our own design; a name that
        // is no sequence number is a file this spool did not write.
        let sequence = path
            .file_stem()
            .and_then(OsStr::to_str)
            .and_then(|it| it.parse::<u64>().ok());
        match (extension, sequence) {
            (Some(STORED), Some(sequence)) => {
                let size = entry.metadata().map_err(io("read", &path))?.len();
                sizes.insert(sequence, size);
            }
            // NOTE: a partial file is a write a crash interrupted, which
            // never counted as stored, so it is removed.
            (Some(PARTIAL), Some(_)) => {
                fs::remove_file(&path).map_err(io("removed", &path))?;
            }
            _ => {
                return Err(SpoolError::Foreign {
                    directory: directory.to_owned(),
                    path,
                });
            }
        }
    }
    Ok(sizes)
}

/// Writes and removes a partial file, so a directory that cannot take a
/// message is refused when it is opened, not when the first message
/// arrives; one a crash leaves behind is removed by the next open.
pub(super) fn probe(directory: &Path) -> Result<(), SpoolError> {
    let path = file(directory, u64::MAX, PARTIAL);
    let handle = private_file(&path)?;
    drop(handle);
    fs::remove_file(&path).map_err(io("removed", &path))
}

/// The path of message `sequence` with `extension`, named so that the file
/// names sort in sequence order.
pub(super) fn file(directory: &Path, sequence: u64, extension: &str) -> PathBuf {
    directory.join(format!("{sequence:020}.{extension}"))
}

/// Writes `message` as message `sequence`: to a partial file flushed to the
/// device, renamed, with the directory flushed after it.
pub(super) fn write_durably(
    directory: &Path,
    sequence: u64,
    message: &SecretSlice<u8>,
) -> Result<(), SpoolError> {
    let partial = file(directory, sequence, PARTIAL);
    let stored = file(directory, sequence, STORED);
    let mut handle = private_file(&partial)?;
    handle
        .write_all(message.expose_secret())
        .map_err(io("written", &partial))?;
    handle.sync_all().map_err(io("written", &partial))?;
    drop(handle);
    fs::rename(&partial, &stored).map_err(io("written", &stored))?;
    sync_directory(directory)
}

#[cfg(unix)]
fn private_file(path: &Path) -> Result<File, SpoolError> {
    use std::os::unix::fs::OpenOptionsExt as _;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(io("created", path))
}

#[cfg(not(unix))]
fn private_file(path: &Path) -> Result<File, SpoolError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io("created", path))
}

#[cfg(unix)]
pub(super) fn sync_directory(directory: &Path) -> Result<(), SpoolError> {
    File::open(directory)
        .and_then(|handle| handle.sync_all())
        .map_err(io("written", directory))
}

// NOTE: no specification governs this: our own design; outside Unix a
// directory cannot be opened to flush it, and the rename is what is relied on.
#[cfg(not(unix))]
pub(super) fn sync_directory(_directory: &Path) -> Result<(), SpoolError> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn create_private(directory: &Path) -> Result<(), SpoolError> {
    use std::os::unix::fs::DirBuilderExt as _;
    if directory.is_dir() {
        return Ok(());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .map_err(io("created", directory))
}

#[cfg(not(unix))]
pub(super) fn create_private(directory: &Path) -> Result<(), SpoolError> {
    fs::create_dir_all(directory).map_err(io("created", directory))
}

#[cfg(unix)]
pub(super) fn check_private(directory: &Path) -> Result<(), SpoolError> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = fs::metadata(directory)
        .map_err(io("read", directory))?
        .permissions()
        .mode();
    let shared = mode & 0o077;
    if shared == 0 {
        Ok(())
    } else {
        Err(SpoolError::Exposed(directory.to_owned()))
    }
}

#[cfg(not(unix))]
pub(super) fn check_private(_directory: &Path) -> Result<(), SpoolError> {
    Ok(())
}

/// The [`SpoolError::Io`] of `action` on `path`.
pub(super) fn io<'a>(
    action: &'static str,
    path: &'a Path,
) -> impl FnOnce(std::io::Error) -> SpoolError + 'a {
    move |source| SpoolError::Io {
        action,
        path: path.to_owned(),
        source,
    }
}
