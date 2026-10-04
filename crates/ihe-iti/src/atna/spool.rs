// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The local store of audit messages awaiting delivery: ITI TF-2
//! §3.20.4.1.1 has a sender that cannot reach its Audit Record Repository
//! store the record locally and send it when it is able.
//!
//! A [`Spool`] is bounded by a number of messages and a number of bytes; a
//! message past either bound is refused, never dropped silently, so its
//! caller can fail the exchange it audits. Messages leave in the order they
//! were stored.
//!
//! On disk ([`Spool::open`]), each message is one file named by its sequence
//! number. It is written to a temporary file, flushed to the device, renamed,
//! and the directory is flushed too, so a stored message survives a crash or
//! a restart. On Unix the directory must give its owner alone access, and
//! every file is created readable by its owner alone, because a message
//! names a patient. In memory ([`Spool::in_memory`]), the messages live as
//! long as the process, for a development deployment.
//!
//! A stored message that cannot be read, or that is no whole RFC 5425 frame,
//! is moved to the `quarantine` subdirectory when the drain reaches it,
//! logged with its sequence number and never its content, and the drain goes
//! on with the next one, so one bad file never holds back the rest. A
//! quarantined message stays counted under the bounds until an operator
//! removes it. A file the spool did not write refuses [`Spool::open`]
//! instead, naming the file to move: in a directory only the gateway's user
//! may enter, it was put there by hand.
//!
//! The bounds and the file layout are our own design: no specification
//! governs them.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use secrecy::{ExposeSecret, SecretSlice};

/// The extension of a stored message.
const STORED: &str = "msg";

/// The extension of a message being written.
const PARTIAL: &str = "partial";

/// The subdirectory a message that cannot be delivered is moved to.
pub const QUARANTINE: &str = "quarantine";

/// The bounds of a spool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// The most messages held at once.
    pub max_messages: usize,
    /// The most bytes held at once, over every message.
    pub max_bytes: u64,
}

/// Why a spool could not be opened or could not store a message.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpoolError {
    /// The spool already holds its most messages or bytes.
    #[error("the audit spool is full: {messages} messages, {bytes} bytes")]
    Full {
        /// The messages it holds.
        messages: usize,
        /// The bytes it holds.
        bytes: u64,
    },
    /// The directory gives a group or other users access to it.
    #[error("the audit spool directory {0} is open to its group or to other users")]
    Exposed(PathBuf),
    /// The directory holds a file the spool did not write.
    #[error(
        "the audit spool directory {} holds {}, which the gateway did not write: move that file out of the spool directory, keeping it if it may be an audit record, and start again",
        directory.display(),
        path.display()
    )]
    Foreign {
        /// The spool directory, or its quarantine.
        directory: PathBuf,
        /// The file to move.
        path: PathBuf,
    },
    /// A file system operation failed.
    #[error("the audit spool at {path} could not be {action}")]
    Io {
        /// What was being done: `read`, `written`, `created`, `removed`.
        action: &'static str,
        /// The path it was done to.
        path: PathBuf,
        /// The cause.
        #[source]
        source: std::io::Error,
    },
    /// The task that touched the disk did not finish.
    #[error("the audit spool task did not finish")]
    Task(#[source] tokio::task::JoinError),
}

/// What a spool holds right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Depth {
    /// The messages held, quarantined ones included.
    pub messages: usize,
    /// The bytes held, quarantined ones included.
    pub bytes: u64,
    /// The messages held in quarantine.
    pub quarantined: usize,
}

impl Depth {
    /// The messages awaiting delivery: those held, less the quarantined.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.messages.saturating_sub(self.quarantined)
    }
}

/// One stored message, with the sequence number it is removed by.
pub struct Stored {
    /// The sequence number.
    pub sequence: u64,
    /// The message.
    pub message: SecretSlice<u8>,
}

impl std::fmt::Debug for Stored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stored")
            .field("sequence", &self.sequence)
            .finish_non_exhaustive()
    }
}

/// A bounded store of messages awaiting delivery.
#[derive(Clone)]
pub struct Spool {
    inner: Arc<Mutex<Inner>>,
}

impl std::fmt::Debug for Spool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("Spool")
            .field("on_disk", &matches!(inner.store, Store::Disk { .. }))
            .field("depth", &inner.depth)
            .field("bounds", &inner.bounds)
            .finish()
    }
}

struct Inner {
    store: Store,
    bounds: Bounds,
    depth: Depth,
    next: u64,
}

enum Store {
    Disk {
        directory: PathBuf,
        sizes: BTreeMap<u64, u64>,
        quarantined: BTreeMap<u64, u64>,
    },
    Memory(VecDeque<(u64, SecretSlice<u8>)>),
}

impl Spool {
    /// The spool in `directory`, created when missing, holding the messages
    /// a previous process left in it.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Exposed`] when the directory or its quarantine gives a
    /// group or other users access to it (Unix), [`SpoolError::Foreign`]
    /// when either holds a file the spool did not write, and
    /// [`SpoolError::Io`] when they cannot be created, read or written.
    pub fn open(directory: &Path, bounds: Bounds) -> Result<Self, SpoolError> {
        let held = directory.join(QUARANTINE);
        create_private(directory)?;
        check_private(directory)?;
        create_private(&held)?;
        check_private(&held)?;
        probe(directory)?;
        let sizes = scan(directory, Some(&held))?;
        let quarantined = scan(&held, None)?;
        let depth = Depth {
            messages: sizes.len().saturating_add(quarantined.len()),
            bytes: sizes
                .values()
                .chain(quarantined.values())
                .fold(0, |sum, size| sum.saturating_add(*size)),
            quarantined: quarantined.len(),
        };
        let next = sizes
            .keys()
            .chain(quarantined.keys())
            .max()
            .map_or(0, |last| last.saturating_add(1));
        Ok(Self::with(
            Store::Disk {
                directory: directory.to_owned(),
                sizes,
                quarantined,
            },
            bounds,
            depth,
            next,
        ))
    }

    /// A spool in memory, lost when the process ends: for a development
    /// deployment.
    #[must_use]
    pub fn in_memory(bounds: Bounds) -> Self {
        Self::with(Store::Memory(VecDeque::new()), bounds, Depth::default(), 0)
    }

    fn with(store: Store, bounds: Bounds, depth: Depth, next: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                store,
                bounds,
                depth,
                next,
            })),
        }
    }

    /// Whether the messages are stored on disk.
    #[must_use]
    pub fn is_durable(&self) -> bool {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        matches!(inner.store, Store::Disk { .. })
    }

    /// What the spool holds.
    #[must_use]
    pub fn depth(&self) -> Depth {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .depth
    }

    /// Stores `message` after every message stored before it. On disk, it
    /// returns once the message is on the device.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Full`] past a bound, and [`SpoolError::Io`] when the
    /// message cannot be written.
    pub async fn push(&self, message: SecretSlice<u8>) -> Result<(), SpoolError> {
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || {
            inner
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(message)
        })
        .await
        .map_err(SpoolError::Task)?
    }

    /// The oldest message that can be delivered, still stored.
    ///
    /// Every older message that cannot be read, or is no whole RFC 5425
    /// frame, is moved to the quarantine on the way.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Io`] when a message can neither be read nor moved to
    /// the quarantine.
    pub async fn oldest(&self) -> Result<Option<Stored>, SpoolError> {
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || {
            inner
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .oldest()
        })
        .await
        .map_err(SpoolError::Task)?
    }

    /// Removes the message `sequence`, once it is delivered.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Io`] when it cannot be removed.
    pub async fn remove(&self, sequence: u64) -> Result<(), SpoolError> {
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || {
            inner
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(sequence)
        })
        .await
        .map_err(SpoolError::Task)?
    }
}

impl Inner {
    fn push(&mut self, message: SecretSlice<u8>) -> Result<(), SpoolError> {
        let size = u64::try_from(message.expose_secret().len()).unwrap_or(u64::MAX);
        let bytes = self.depth.bytes.saturating_add(size);
        if self.depth.messages >= self.bounds.max_messages || bytes > self.bounds.max_bytes {
            return Err(SpoolError::Full {
                messages: self.depth.messages,
                bytes: self.depth.bytes,
            });
        }
        let sequence = self.next;
        match &mut self.store {
            Store::Disk {
                directory, sizes, ..
            } => {
                write_durably(directory, sequence, &message)?;
                sizes.insert(sequence, size);
            }
            Store::Memory(queue) => queue.push_back((sequence, message)),
        }
        self.next = sequence.saturating_add(1);
        self.depth.messages = self.depth.messages.saturating_add(1);
        self.depth.bytes = bytes;
        Ok(())
    }

    fn oldest(&mut self) -> Result<Option<Stored>, SpoolError> {
        loop {
            let (directory, sequence) = match &self.store {
                Store::Memory(queue) => {
                    return Ok(queue.front().map(|(sequence, message)| Stored {
                        sequence: *sequence,
                        message: SecretSlice::from(message.expose_secret().to_vec()),
                    }));
                }
                Store::Disk {
                    directory, sizes, ..
                } => match sizes.keys().next() {
                    None => return Ok(None),
                    Some(sequence) => (directory.clone(), *sequence),
                },
            };
            match fs::read(file(&directory, sequence, STORED)) {
                Ok(message) if super::syslog::is_frame(&message) => {
                    return Ok(Some(Stored {
                        sequence,
                        message: SecretSlice::from(message),
                    }));
                }
                Ok(_) => self.quarantine(&directory, sequence, "it is no whole RFC 5425 frame")?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tracing::error!(sequence, "a spooled audit message is gone from the spool");
                    self.forget(sequence);
                }
                Err(error) => {
                    tracing::error!(sequence, error = %error, "a spooled audit message cannot be read");
                    self.quarantine(&directory, sequence, "it cannot be read")?;
                }
            }
        }
    }

    /// Drops message `sequence`, whose file is gone, from the count.
    fn forget(&mut self, sequence: u64) {
        if let Store::Disk { sizes, .. } = &mut self.store
            && let Some(size) = sizes.remove(&sequence)
        {
            self.depth.messages = self.depth.messages.saturating_sub(1);
            self.depth.bytes = self.depth.bytes.saturating_sub(size);
        }
    }

    /// Moves message `sequence` to the quarantine, where it stays counted.
    fn quarantine(
        &mut self,
        directory: &Path,
        sequence: u64,
        why: &'static str,
    ) -> Result<(), SpoolError> {
        let held = directory.join(QUARANTINE);
        let from = file(directory, sequence, STORED);
        let to = file(&held, sequence, STORED);
        fs::rename(&from, &to).map_err(io("moved to the quarantine", &from))?;
        sync_directory(directory)?;
        sync_directory(&held)?;
        if let Store::Disk {
            sizes, quarantined, ..
        } = &mut self.store
            && let Some(size) = sizes.remove(&sequence)
        {
            quarantined.insert(sequence, size);
            self.depth.quarantined = self.depth.quarantined.saturating_add(1);
        }
        // NOTE: RFC 5425 §4.3 carries no application-level acknowledgement, so
        // only a file the spool cannot send is quarantined, never a rejection.
        tracing::error!(
            sequence,
            reason = why,
            "a spooled audit message was moved to the quarantine; the drain goes on"
        );
        Ok(())
    }

    fn remove(&mut self, sequence: u64) -> Result<(), SpoolError> {
        let size = match &mut self.store {
            Store::Disk {
                directory, sizes, ..
            } => {
                let Some(size) = sizes.get(&sequence).copied() else {
                    return Ok(());
                };
                let path = file(directory, sequence, STORED);
                fs::remove_file(&path).map_err(io("removed", &path))?;
                sizes.remove(&sequence);
                size
            }
            Store::Memory(queue) => {
                let Some(position) = queue.iter().position(|(held, _)| *held == sequence) else {
                    return Ok(());
                };
                queue.remove(position).map_or(0, |(_, message)| {
                    u64::try_from(message.expose_secret().len()).unwrap_or(u64::MAX)
                })
            }
        };
        self.depth.messages = self.depth.messages.saturating_sub(1);
        self.depth.bytes = self.depth.bytes.saturating_sub(size);
        Ok(())
    }
}

/// The messages in `directory`, by sequence number, with their sizes; a
/// partial write a crash left is removed, and `skip`, the quarantine, is
/// passed over.
fn scan(directory: &Path, skip: Option<&Path>) -> Result<BTreeMap<u64, u64>, SpoolError> {
    let mut sizes = BTreeMap::new();
    for entry in fs::read_dir(directory).map_err(io("read", directory))? {
        let entry = entry.map_err(io("read", directory))?;
        let path = entry.path();
        if Some(path.as_path()) == skip {
            continue;
        }
        let extension = path.extension().and_then(|it| it.to_str());
        // NOTE: no specification governs this: our own design; a name that
        // is no sequence number is a file this spool did not write.
        let sequence = path
            .file_stem()
            .and_then(|it| it.to_str())
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
fn probe(directory: &Path) -> Result<(), SpoolError> {
    let path = file(directory, u64::MAX, PARTIAL);
    let handle = private_file(&path)?;
    drop(handle);
    fs::remove_file(&path).map_err(io("removed", &path))
}

/// The path of message `sequence` with `extension`, named so that the file
/// names sort in sequence order.
fn file(directory: &Path, sequence: u64, extension: &str) -> PathBuf {
    directory.join(format!("{sequence:020}.{extension}"))
}

/// Writes `message` as message `sequence`: to a partial file flushed to the
/// device, renamed, with the directory flushed after it.
fn write_durably(
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
fn sync_directory(directory: &Path) -> Result<(), SpoolError> {
    File::open(directory)
        .and_then(|handle| handle.sync_all())
        .map_err(io("written", directory))
}

// NOTE: no specification governs this: our own design; outside Unix a
// directory cannot be opened to flush it, and the rename is what is relied on.
#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> Result<(), SpoolError> {
    Ok(())
}

#[cfg(unix)]
fn create_private(directory: &Path) -> Result<(), SpoolError> {
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
fn create_private(directory: &Path) -> Result<(), SpoolError> {
    fs::create_dir_all(directory).map_err(io("created", directory))
}

#[cfg(unix)]
fn check_private(directory: &Path) -> Result<(), SpoolError> {
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
fn check_private(_directory: &Path) -> Result<(), SpoolError> {
    Ok(())
}

/// The [`SpoolError::Io`] of `action` on `path`.
fn io<'a>(action: &'static str, path: &'a Path) -> impl FnOnce(std::io::Error) -> SpoolError + 'a {
    move |source| SpoolError::Io {
        action,
        path: path.to_owned(),
        source,
    }
}
