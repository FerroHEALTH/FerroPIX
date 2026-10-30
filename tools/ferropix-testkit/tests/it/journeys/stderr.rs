// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What the test process writes to its standard error while a journey runs,
//! which is where the console's server, run in process, reports a panic.
//! The descriptor is passed through a pipe: every byte still reaches the
//! original standard error, and the journey reads the copy once it ends.
//! The standard library's panic hook writes to the descriptor, which reaches
//! it under nextest, the suite's runner, since nextest runs every test
//! binary with `--nocapture`. No specification governs this: our own design.

use std::fs::File;
use std::io::{Read as _, Write as _};
use std::os::fd::{AsFd as _, OwnedFd};
use std::thread::JoinHandle;

use tokio::sync::{Mutex, MutexGuard};

/// The words the standard library's panic hook writes for a panic
/// (`std::panic`, the default hook: "thread '…' panicked at …").
pub(crate) const SERVER_PANIC: &str = "panicked at";

/// One copy at a time in a process, so two journeys under one runner never
/// swap the descriptor under each other.
static ONE_AT_A_TIME: Mutex<()> = Mutex::const_new(());

/// The standard error of the test process, passed through a pipe that keeps
/// a copy until [`Stderr::end`].
#[derive(Debug)]
pub(crate) struct Stderr {
    /// The original standard error, put back at the end.
    original: Option<OwnedFd>,
    /// The thread that forwards every byte and keeps the copy.
    forward: Option<JoinHandle<Vec<u8>>>,
    /// The process's one turn.
    _turn: MutexGuard<'static, ()>,
}

impl Stderr {
    /// Starts copying the process's standard error.
    pub(crate) async fn copy() -> Self {
        let turn = ONE_AT_A_TIME.lock().await;
        let original = std::io::stderr()
            .as_fd()
            .try_clone_to_owned()
            .expect("the standard error can be duplicated");
        let mut out = File::from(
            original
                .try_clone()
                .expect("the standard error can be duplicated"),
        );
        let (reader, writer) = rustix::pipe::pipe().expect("a pipe opens");
        rustix::stdio::dup2_stderr(&writer).expect("the pipe takes the standard error");
        drop(writer);
        let forward = std::thread::spawn(move || {
            let mut reader = File::from(reader);
            let mut kept = Vec::new();
            let mut chunk = [0_u8; 8192];
            while let Ok(read) = reader.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                out.write_all(&chunk[..read])
                    .expect("the original standard error takes the bytes");
                kept.extend_from_slice(&chunk[..read]);
            }
            kept
        });
        Self {
            original: Some(original),
            forward: Some(forward),
            _turn: turn,
        }
    }

    /// Puts the original standard error back and returns everything written
    /// to it since [`Stderr::copy`].
    pub(crate) fn end(mut self) -> String {
        self.restore()
    }

    /// Puts the original back, which closes the pipe's last writer, and
    /// takes the copy once the forwarding thread has passed every byte on.
    fn restore(&mut self) -> String {
        let Some(original) = self.original.take() else {
            return String::new();
        };
        rustix::stdio::dup2_stderr(&original).expect("the standard error comes back");
        let kept = self
            .forward
            .take()
            .map(|forward| forward.join().expect("the forwarding thread ends"))
            .unwrap_or_default();
        String::from_utf8_lossy(&kept).into_owned()
    }
}

impl Drop for Stderr {
    fn drop(&mut self) {
        drop(self.restore());
    }
}

#[tokio::test]
async fn the_copy_holds_what_the_process_wrote_and_the_descriptor_comes_back() {
    let copy = Stderr::copy().await;
    std::io::stderr()
        .write_all(format!("a journey's line naming {SERVER_PANIC}\n").as_bytes())
        .expect("the standard error takes a line");
    let written = copy.end();
    assert!(
        written.contains(SERVER_PANIC),
        "the copy holds the line written while it ran: {written:?}"
    );
    let again = Stderr::copy().await;
    assert!(
        again.end().is_empty(),
        "a second copy starts empty, so the first put the descriptor back"
    );
}
