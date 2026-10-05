// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The local store of audit messages awaiting delivery: ITI TF-2
//! §3.20.4.1.1 has a sender that cannot reach its Audit Record Repository
//! store the record locally and send it when it is able.
//!
//! A [`Spool`] is bounded by a number of messages and a number of bytes,
//! the messages queued for a write counted with those stored, so a stalled
//! disk holds no more in memory than the bounds admit; a message past either
//! bound is refused at once, never dropped silently, so its caller can fail
//! the exchange it audits. Storing one message is bounded in
//! time too: one not stored within [`Bounds::write_timeout`], the wait for a
//! write before it included, is refused with [`SpoolError::Late`] and is not
//! counted. That write goes on, or still waits its turn, and once it ends
//! the message is stored and delivered like any other, or its failure is
//! logged, so a slow disk neither loses nor repeats a message. Messages
//! leave in the order they were stored.
//!
//! On disk ([`Spool::open`]), each message is one file named by its sequence
//! number. It is written to a temporary file, flushed to the device, renamed,
//! and the directory is flushed too, so a stored message survives a crash or
//! a restart. On Unix the directory must give its owner alone access, and
//! every file is created readable by its owner alone, because a message
//! names a patient. In memory ([`Spool::in_memory`]), the messages live as
//! long as the process, for a development deployment.
//!
//! A spool holds one [`Content`]: RFC 5425 syslog frames, or, with the
//! `balp` feature, FHIR `AuditEvent` records for the FHIR Feed. A stored
//! message that cannot be read, or that is no message of the spool's content,
//! is moved to the `quarantine` subdirectory when the drain reaches it,
//! logged with its sequence number and never its content, and the drain goes
//! on with the next one, so one bad file never holds back the rest. So is a
//! message the repository refused ([`Spool::reject`]). A quarantined message
//! stays counted under the bounds until an operator removes it; in memory,
//! it stays held until the process ends. A file the spool did not write refuses [`Spool::open`]
//! instead, naming the file to move: in a directory only the gateway's user
//! may enter, it was put there by hand.
//!
//! The bounds and the file layout are our own design: no specification
//! governs them.

mod disk;
pub(super) mod in_flight;

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use secrecy::{ExposeSecret, SecretSlice};
use tokio::sync::Notify;
use tokio::task::JoinError;

use disk::{check_private, create_private, file, io, probe, scan, sync_directory, write_durably};
use in_flight::InFlight;

/// The extension of a stored message.
const STORED: &str = "msg";

/// The extension of a message being written.
const PARTIAL: &str = "partial";

/// The subdirectory a message that cannot be delivered is moved to.
pub const QUARANTINE: &str = "quarantine";

/// What a spool on disk holds, which a stored file is read back as.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Content {
    /// RFC 5425 syslog frames, each a DICOM audit message (ITI TF-2
    /// §3.20.4.1).
    #[default]
    SyslogFrames,
    /// FHIR `AuditEvent` resources as JSON, for the FHIR Feed (ITI TF-2
    /// §3.20.4.2).
    #[cfg(feature = "balp")]
    AuditEvents,
}

impl Content {
    /// Whether `bytes` are one message of this content.
    fn holds(self, bytes: &[u8]) -> bool {
        match self {
            Self::SyslogFrames => super::syslog::is_frame(bytes),
            #[cfg(feature = "balp")]
            Self::AuditEvents => {
                serde_json::from_slice::<fhir_types::r4::audit_event::AuditEvent>(bytes).is_ok()
            }
        }
    }

    /// What a stored file of another content is, as the quarantine logs it.
    fn mismatch(self) -> &'static str {
        match self {
            Self::SyslogFrames => "it is no whole RFC 5425 frame",
            #[cfg(feature = "balp")]
            Self::AuditEvents => "it is no FHIR AuditEvent",
        }
    }
}

/// The bounds of a spool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// The most messages held at once.
    pub max_messages: usize,
    /// The most bytes held at once, over every message.
    pub max_bytes: u64,
    /// The longest storing one message may take, the wait for a write
    /// before it included.
    pub write_timeout: Duration,
}

/// Why a spool could not be opened or could not store a message.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpoolError {
    /// The spool already holds its most messages or bytes, the messages
    /// queued for a write counted with those stored.
    #[error("the audit spool is full: {messages} messages, {bytes} bytes")]
    Full {
        /// The messages it holds or has queued.
        messages: usize,
        /// The bytes it holds or has queued.
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
    /// The message was not stored within [`Bounds::write_timeout`]. Its write
    /// goes on, and a message it stores is delivered like any other.
    #[error("the audit spool did not store the message within {bound:?}")]
    Late {
        /// The bound it missed.
        bound: Duration,
    },
    /// The task that touched the disk did not finish.
    #[error("the audit spool task did not finish")]
    Task(#[source] JoinError),
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
///
/// Reading its state never waits on the disk, so a write that stalls holds
/// up no reader of [`Spool::depth`].
#[derive(Clone)]
pub struct Spool {
    inner: Arc<Mutex<Inner>>,
    shown: Arc<Mutex<Ledger>>,
    turn: Arc<tokio::sync::Mutex<()>>,
    arrived: Arc<Notify>,
    bounds: Bounds,
    durable: bool,
}

impl std::fmt::Debug for Spool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spool")
            .field("on_disk", &self.durable)
            .field("depth", &self.depth())
            .field("bounds", &self.bounds)
            .finish_non_exhaustive()
    }
}

struct Inner {
    store: Store,
    bounds: Bounds,
    depth: Depth,
    shown: Arc<Mutex<Ledger>>,
    next: u64,
}

/// What readers of the spool see without taking its lock: the depth it
/// holds, the messages queued for a write, and the refusals.
#[derive(Debug, Default)]
struct Ledger {
    depth: Depth,
    queued: usize,
    queued_bytes: u64,
    refused: u64,
}

impl Ledger {
    /// Counts a message refused for want of room.
    fn refuse(&mut self) -> u64 {
        self.refused = self.refused.saturating_add(1);
        self.refused
    }
}

/// The place a message holds under the bounds while it waits for its
/// write, given back once the write has ended and the depth shows it.
struct Place {
    ledger: Arc<Mutex<Ledger>>,
    size: u64,
}

impl Drop for Place {
    fn drop(&mut self) {
        let mut ledger = lock(&self.ledger);
        ledger.queued = ledger.queued.saturating_sub(1);
        ledger.queued_bytes = ledger.queued_bytes.saturating_sub(self.size);
    }
}

/// Locks `mutex`, taking over the state a holder that panicked left.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

enum Store {
    Disk {
        directory: PathBuf,
        content: Content,
        sizes: BTreeMap<u64, u64>,
        quarantined: BTreeMap<u64, u64>,
    },
    Memory {
        queue: VecDeque<(u64, SecretSlice<u8>)>,
        held: Vec<(u64, SecretSlice<u8>)>,
    },
}

impl Spool {
    /// The spool of syslog frames in `directory`, created when missing,
    /// holding the messages a previous process left in it.
    ///
    /// # Errors
    ///
    /// The errors of [`Spool::open_for`].
    pub fn open(directory: &Path, bounds: Bounds) -> Result<Self, SpoolError> {
        Self::open_for(directory, bounds, Content::SyslogFrames)
    }

    /// The spool of `content` in `directory`, created when missing, holding
    /// the messages a previous process left in it.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Exposed`] when the directory or its quarantine gives a
    /// group or other users access to it (Unix), [`SpoolError::Foreign`]
    /// when either holds a file the spool did not write, and
    /// [`SpoolError::Io`] when they cannot be created, read or written.
    pub fn open_for(
        directory: &Path,
        bounds: Bounds,
        content: Content,
    ) -> Result<Self, SpoolError> {
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
                content,
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
        Self::with(
            Store::Memory {
                queue: VecDeque::new(),
                held: Vec::new(),
            },
            bounds,
            Depth::default(),
            0,
        )
    }

    fn with(store: Store, bounds: Bounds, depth: Depth, next: u64) -> Self {
        let durable = matches!(store, Store::Disk { .. });
        let shown = Arc::new(Mutex::new(Ledger {
            depth,
            ..Ledger::default()
        }));
        Self {
            inner: Arc::new(Mutex::new(Inner {
                store,
                bounds,
                depth,
                shown: Arc::clone(&shown),
                next,
            })),
            shown,
            turn: Arc::new(tokio::sync::Mutex::new(())),
            arrived: Arc::new(Notify::new()),
            bounds,
            durable,
        }
    }

    /// Whether the messages are stored on disk.
    #[must_use]
    pub fn is_durable(&self) -> bool {
        self.durable
    }

    /// What the spool holds: a message counts once it is stored.
    #[must_use]
    pub fn depth(&self) -> Depth {
        lock(&self.shown).depth
    }

    /// The messages waiting for their write, which hold their place under
    /// the bounds until it ends.
    #[must_use]
    pub fn queued(&self) -> usize {
        lock(&self.shown).queued
    }

    /// The messages refused with [`SpoolError::Full`] since the spool was
    /// opened.
    #[must_use]
    pub fn refused(&self) -> u64 {
        lock(&self.shown).refused
    }

    /// Stores `message` after every message stored before it. On disk, it
    /// returns once the message is on the device.
    ///
    /// The bounds count the messages queued for a write with those stored,
    /// so a message they have no room for is refused at once, and a stalled
    /// disk holds at most the bounds in memory too. It waits at most
    /// [`Bounds::write_timeout`], the wait for a write before it included. A
    /// message not stored by then is refused with [`SpoolError::Late`] and
    /// is not counted as stored. Its write goes on, or waits on for its
    /// turn: once it ends, the message is stored and delivered like any
    /// other, or the failure is logged with no part of the message.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Full`] past a bound, [`SpoolError::Late`] past
    /// [`Bounds::write_timeout`], and [`SpoolError::Io`] when the message cannot
    /// be written.
    pub async fn push(&self, message: SecretSlice<u8>) -> Result<(), SpoolError> {
        let size = u64::try_from(message.expose_secret().len()).unwrap_or(u64::MAX);
        let place = self.admit(size)?;
        let bound = self.bounds.write_timeout;
        let store = self.store(message, place);
        match tokio::time::Instant::now().checked_add(bound) {
            Some(deadline) => tokio::time::timeout_at(deadline, store)
                .await
                .map_err(|_elapsed| SpoolError::Late { bound })?,
            None => store.await,
        }
    }

    /// Waits until a message is stored, returning at once when one was
    /// stored since the last wait ended.
    pub async fn stored(&self) {
        self.arrived.notified().await;
    }

    /// Stores `message` once every write before it has ended, so a stalled
    /// write holds one blocking thread however many messages wait behind it.
    ///
    /// The write is its own task from the start, waiting for its turn there:
    /// a caller that stops waiting while the message is still queued leaves
    /// it queued, and it is written once the writes before it end.
    // NOTE: ITI TF-2 §3.20.4.1.1 has a record stored locally and sent when the sender
    // is able, so a queued record its exchange gave up on is still written, never dropped.
    async fn store(&self, message: SecretSlice<u8>, place: Place) -> Result<(), SpoolError> {
        let turn = Arc::clone(&self.turn);
        let inner = Arc::clone(&self.inner);
        let arrived = Arc::clone(&self.arrived);
        let write = InFlight::new(tokio::spawn(async move {
            let turn = turn.lock_owned().await;
            tokio::task::spawn_blocking(move || {
                let stored = {
                    let mut inner = lock(&inner);
                    let stored = inner.push(message);
                    inner.publish();
                    stored
                };
                drop(place);
                drop(turn);
                if stored.is_ok() {
                    arrived.notify_one();
                }
                stored
            })
            .await
            .map_err(SpoolError::Task)?
        }));
        write.await.map_err(SpoolError::Task)?.map(|_sequence| ())
    }

    /// Gives a message of `size` bytes its place under the bounds, counting
    /// the messages queued for a write with those stored, or refuses it.
    fn admit(&self, size: u64) -> Result<Place, SpoolError> {
        let mut ledger = lock(&self.shown);
        let messages = ledger.depth.messages.saturating_add(ledger.queued);
        let bytes = ledger.depth.bytes.saturating_add(ledger.queued_bytes);
        if messages >= self.bounds.max_messages
            || bytes.saturating_add(size) > self.bounds.max_bytes
        {
            let refused = ledger.refuse();
            drop(ledger);
            tracing::warn!(
                refused,
                messages,
                bytes,
                "an audit message was refused: the spool and the writes queued for it are at their bounds"
            );
            return Err(SpoolError::Full { messages, bytes });
        }
        ledger.queued = ledger.queued.saturating_add(1);
        ledger.queued_bytes = ledger.queued_bytes.saturating_add(size);
        Ok(Place {
            ledger: Arc::clone(&self.shown),
            size,
        })
    }

    /// Runs `step` on the spool on a blocking thread, and shows the depth it
    /// leaves.
    async fn blocking<T: Send + 'static>(
        &self,
        step: impl FnOnce(&mut Inner) -> Result<T, SpoolError> + Send + 'static,
    ) -> Result<T, SpoolError> {
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || {
            let mut inner = lock(&inner);
            let done = step(&mut inner);
            inner.publish();
            done
        })
        .await
        .map_err(SpoolError::Task)?
    }

    /// The oldest message that can be delivered, still stored.
    ///
    /// Every older message that cannot be read, or is no message of the
    /// spool's [`Content`], is moved to the quarantine on the way.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Io`] when a message can neither be read nor moved to
    /// the quarantine.
    pub async fn oldest(&self) -> Result<Option<Stored>, SpoolError> {
        self.blocking(Inner::oldest).await
    }

    /// Moves the message `sequence`, which the repository refused, to the
    /// quarantine, where it stays counted, so the drain goes on.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Io`] when it cannot be moved.
    pub async fn reject(&self, sequence: u64) -> Result<(), SpoolError> {
        self.blocking(move |inner| inner.reject(sequence)).await
    }

    /// Removes the message `sequence`, once it is delivered.
    ///
    /// # Errors
    ///
    /// [`SpoolError::Io`] when it cannot be removed.
    pub async fn remove(&self, sequence: u64) -> Result<(), SpoolError> {
        self.blocking(move |inner| inner.remove(sequence)).await
    }
}

impl Inner {
    /// Shows the depth to readers that do not take the spool's lock.
    fn publish(&self) {
        lock(&self.shown).depth = self.depth;
    }

    fn push(&mut self, message: SecretSlice<u8>) -> Result<u64, SpoolError> {
        let size = u64::try_from(message.expose_secret().len()).unwrap_or(u64::MAX);
        let bytes = self.depth.bytes.saturating_add(size);
        if self.depth.messages >= self.bounds.max_messages || bytes > self.bounds.max_bytes {
            let refused = lock(&self.shown).refuse();
            tracing::warn!(
                refused,
                messages = self.depth.messages,
                bytes = self.depth.bytes,
                "an audit message was refused: the spool is at its bounds"
            );
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
            Store::Memory { queue, .. } => queue.push_back((sequence, message)),
        }
        self.next = sequence.saturating_add(1);
        self.depth.messages = self.depth.messages.saturating_add(1);
        self.depth.bytes = bytes;
        Ok(sequence)
    }

    fn oldest(&mut self) -> Result<Option<Stored>, SpoolError> {
        loop {
            let (directory, content, sequence) = match &self.store {
                Store::Memory { queue, .. } => {
                    return Ok(queue.front().map(|(sequence, message)| Stored {
                        sequence: *sequence,
                        message: SecretSlice::from(message.expose_secret().to_vec()),
                    }));
                }
                Store::Disk {
                    directory,
                    content,
                    sizes,
                    ..
                } => match sizes.keys().next() {
                    None => return Ok(None),
                    Some(sequence) => (directory.clone(), *content, *sequence),
                },
            };
            match fs::read(file(&directory, sequence, STORED)) {
                Ok(message) if content.holds(&message) => {
                    return Ok(Some(Stored {
                        sequence,
                        message: SecretSlice::from(message),
                    }));
                }
                Ok(_) => self.quarantine(&directory, sequence, content.mismatch())?,
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
        tracing::error!(
            sequence,
            reason = why,
            "a spooled audit message was moved to the quarantine; the drain goes on"
        );
        Ok(())
    }

    fn reject(&mut self, sequence: u64) -> Result<(), SpoolError> {
        match &mut self.store {
            Store::Disk {
                directory, sizes, ..
            } => {
                if !sizes.contains_key(&sequence) {
                    return Ok(());
                }
                let directory = directory.clone();
                self.quarantine(&directory, sequence, "the audit repository refused it")
            }
            Store::Memory { queue, held } => {
                let Some(position) = queue.iter().position(|(kept, _)| *kept == sequence) else {
                    return Ok(());
                };
                if let Some(message) = queue.remove(position) {
                    held.push(message);
                    self.depth.quarantined = self.depth.quarantined.saturating_add(1);
                    tracing::error!(
                        sequence,
                        reason = "the audit repository refused it",
                        "an audit message held in memory was set aside; the drain goes on"
                    );
                }
                Ok(())
            }
        }
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
            Store::Memory { queue, .. } => {
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
