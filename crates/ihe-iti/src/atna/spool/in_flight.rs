// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A write to the spool its caller may stop waiting for.
//!
//! The write runs on a blocking thread and cannot be stopped there, so a
//! caller that gives up on it leaves it running: once it ends, the message
//! it stored is delivered like any other, and a write that failed is logged
//! by its error, never by its message.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::task::{JoinError, JoinHandle};

use super::SpoolError;

/// A write in flight, ending with the sequence number it stored.
///
/// Dropped before it ends, because its caller stopped waiting, it leaves a
/// task that logs how the write ended.
pub(super) struct InFlight(Option<JoinHandle<Result<u64, SpoolError>>>);

impl InFlight {
    /// The write `write` runs.
    pub(super) fn new(write: JoinHandle<Result<u64, SpoolError>>) -> Self {
        Self(Some(write))
    }
}

impl Future for InFlight {
    type Output = Result<Result<u64, SpoolError>, JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(write) = self.0.as_mut() else {
            return Poll::Pending;
        };
        let ended = std::task::ready!(Pin::new(write).poll(cx));
        self.0 = None;
        Poll::Ready(ended)
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if let Some(write) = self.0.take() {
            abandoned(write);
        }
    }
}

// NOTE: ITI TF-2 §3.20.4.1.1 has a record stored locally sent when the sender is
// able, so a write its exchange stopped waiting for is delivered, never discarded.
fn abandoned(write: JoinHandle<Result<u64, SpoolError>>) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        tracing::error!(
            "an audit message write was given up outside a runtime; a message it stores is delivered"
        );
        return;
    };
    drop(runtime.spawn(async move {
        match write.await {
            Ok(Ok(sequence)) => tracing::warn!(
                sequence,
                "an audit message was stored after its exchange stopped waiting for it; it is delivered like any other"
            ),
            Ok(Err(error)) => tracing::error!(
                error = %super::super::chain(&error),
                "an audit message its exchange stopped waiting for could not be stored"
            ),
            Err(error) => tracing::error!(
                error = %error,
                "the task storing an audit message its exchange stopped waiting for did not end"
            ),
        }
    }));
}

#[cfg(test)]
pub(in crate::atna) mod tests {
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use secrecy::{ExposeSecret as _, SecretSlice};
    use tracing_subscriber::fmt::MakeWriter;

    use super::super::{Bounds, Depth, Spool, SpoolError, lock};

    /// The bound the stalled writes miss.
    pub(in crate::atna) const BOUND: Duration = Duration::from_millis(200);

    /// The time a loaded host may add to any wait a test makes.
    pub(in crate::atna) const SLACK: Duration = Duration::from_secs(3);

    /// A synthetic patient identifier the messages carry and no log may.
    pub(in crate::atna) const PATIENT: &str = "2.999.512.1^^^PAT-512-0001";

    /// A write that does not end: the spool's lock, held on another thread
    /// as a stalled disk holds the write that has it, until released.
    pub(in crate::atna) struct Stall {
        release: mpsc::Sender<()>,
        holder: JoinHandle<()>,
    }

    impl Stall {
        /// Stalls every write to `spool`.
        pub(in crate::atna) fn of(spool: &Spool) -> Self {
            let inner = Arc::clone(&spool.inner);
            let (held, holding) = mpsc::channel();
            let (release, released) = mpsc::channel::<()>();
            let holder = std::thread::spawn(move || {
                let guard = lock(&inner);
                held.send(()).expect("the test waits for the stall");
                released.recv().expect("the test releases the stall");
                drop(guard);
            });
            holding.recv().expect("the stall holds the spool");
            Self { release, holder }
        }

        /// Lets the stalled write run.
        pub(in crate::atna) fn release(self) {
            self.release.send(()).expect("the stall waits");
            self.holder.join().expect("the stall ends");
        }
    }

    /// Every log line written while it is the default subscriber.
    #[derive(Clone, Default)]
    pub(in crate::atna) struct Logs(Arc<Mutex<Vec<u8>>>);

    impl Logs {
        /// The log, as text.
        pub(in crate::atna) fn text(&self) -> String {
            String::from_utf8_lossy(&lock(&self.0)).into_owned()
        }

        /// Waits at most [`SLACK`] for a line holding `needle`.
        pub(in crate::atna) async fn wait_for(&self, needle: &str) -> String {
            let until = Instant::now() + SLACK;
            while !self.text().contains(needle) && Instant::now() < until {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            self.text()
        }

        /// The subscriber that writes here.
        pub(in crate::atna) fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync {
            tracing_subscriber::fmt()
                .with_writer(self.clone())
                .with_ansi(false)
                .with_max_level(tracing::Level::TRACE)
                .finish()
        }
    }

    /// One writer of [`Logs`].
    pub(in crate::atna) struct LogsWriter(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for LogsWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            lock(&self.0).write(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Logs {
        type Writer = LogsWriter;

        fn make_writer(&'a self) -> Self::Writer {
            LogsWriter(Arc::clone(&self.0))
        }
    }

    /// A message that names [`PATIENT`].
    pub(in crate::atna) fn naming_the_patient() -> SecretSlice<u8> {
        let text = format!("<AuditMessage>{PATIENT}</AuditMessage>");
        SecretSlice::from(format!("{} {text}", text.len()).into_bytes())
    }

    fn bounds() -> Bounds {
        Bounds {
            max_messages: 16,
            max_bytes: 1 << 20,
            write_timeout: BOUND,
        }
    }

    #[tokio::test]
    async fn a_stalled_write_is_refused_within_its_bound_and_stored_once_when_it_ends() {
        let logs = Logs::default();
        let _logging = tracing::subscriber::set_default(logs.subscriber());
        let directory = tempfile::tempdir().expect("a directory");
        let spool = Spool::open(&directory.path().join("spool"), bounds()).expect("it opens");
        let stall = Stall::of(&spool);

        let asked = Instant::now();
        let refused = spool.push(naming_the_patient()).await;
        assert!(asked.elapsed() < BOUND + SLACK, "{:?}", asked.elapsed());
        assert!(
            matches!(refused, Err(SpoolError::Late { bound }) if bound == BOUND),
            "{refused:?}"
        );
        assert_eq!(Depth::default(), spool.depth(), "nothing counts as stored");

        let behind = Instant::now();
        let queued = spool.push(naming_the_patient()).await;
        assert!(behind.elapsed() < BOUND + SLACK, "{:?}", behind.elapsed());
        assert!(matches!(queued, Err(SpoolError::Late { .. })), "{queued:?}");
        assert_eq!(Depth::default(), spool.depth());

        stall.release();
        tokio::time::timeout(SLACK, spool.stored())
            .await
            .expect("the stalled write stores its message once it ends");
        assert_eq!(1, spool.depth().messages, "stored once, never twice");
        let stored = spool.oldest().await.expect("it reads").expect("one");
        assert!(
            String::from_utf8_lossy(stored.message.expose_secret()).contains(PATIENT),
            "the message the exchange gave up on"
        );
        spool.remove(stored.sequence).await.expect("it removes");
        assert!(spool.oldest().await.expect("it reads").is_none());

        let log = logs
            .wait_for("stored after its exchange stopped waiting")
            .await;
        assert!(
            log.contains("stored after its exchange stopped waiting"),
            "{log}"
        );
        assert!(!log.contains(PATIENT), "no log names the patient: {log}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_stalled_write_that_then_fails_is_logged_and_counted_nowhere() {
        use std::os::unix::fs::PermissionsExt as _;

        let logs = Logs::default();
        let _logging = tracing::subscriber::set_default(logs.subscriber());
        let directory = tempfile::tempdir().expect("a directory");
        let path = directory.path().join("spool");
        let spool = Spool::open(&path, bounds()).expect("it opens");
        let stall = Stall::of(&spool);

        let refused = spool.push(naming_the_patient()).await;
        assert!(
            matches!(refused, Err(SpoolError::Late { .. })),
            "{refused:?}"
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).expect("chmod");
        stall.release();

        let log = logs.wait_for("could not be stored").await;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        assert!(log.contains("could not be stored"), "{log}");
        assert!(!log.contains(PATIENT), "no log names the patient: {log}");
        assert_eq!(Depth::default(), spool.depth());
        assert!(spool.oldest().await.expect("it reads").is_none());
    }
}
