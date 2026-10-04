// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The waits a test of a timeout or a deadline makes, sized so that how busy
//! the host is cannot change which side of a deadline an event falls on.
//!
//! The clients and the forwarder talk to their peers over real sockets, so
//! the tokio clock cannot be paused under them: a paused clock advances to
//! the next timer whenever the runtime is idle, even while bytes are still
//! in flight. A deadline is taken after the client or the forwarder and its
//! peer are set up, and sits the waits their own timeouts allow plus
//! [`SLACK`] past the moment it is taken; a peer meant to stay silent delays
//! its answer by [`SILENT`], past the life of any test. No specification
//! governs this: our own design.

use std::time::Duration;
#[cfg(any(feature = "atna", feature = "mcsd"))]
use std::time::Instant;

/// The delay of a peer that never answers inside a test.
#[cfg(any(feature = "pixm", feature = "pdqm", feature = "xcpd"))]
pub(crate) const SILENT: Duration = Duration::from_hours(1);

/// The time a loaded host may add to any step a test waits on.
pub(crate) const SLACK: Duration = Duration::from_secs(3);

/// The instant `waits` plus [`SLACK`] from now.
#[cfg(any(feature = "atna", feature = "mcsd"))]
pub(crate) fn deadline(waits: Duration) -> Instant {
    Instant::now()
        .checked_add(waits.saturating_add(SLACK))
        .expect("the deadline should be within the platform clock")
}

/// The output of `call`, which its own timeout `limit` bounds, failing the
/// test when `call` outlasts `limit` plus [`SLACK`].
#[cfg(any(feature = "pixm", feature = "pdqm", feature = "xcpd"))]
pub(crate) async fn bounded<T>(limit: Duration, call: impl Future<Output = T>) -> T {
    tokio::time::timeout(limit.saturating_add(SLACK), call)
        .await
        .expect("the call should end within its own timeout plus the slack")
}
