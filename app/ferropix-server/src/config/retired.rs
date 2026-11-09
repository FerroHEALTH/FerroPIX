// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The keys of FerroPIX's own sections that v0.0.24 renamed, each refused by
//! name for that one release.
//!
//! v0.0.24 writes every span of time and size as `ferrohealth-config`'s
//! units, a string of a whole number and its unit under a key that names
//! none (`timeout = "10s"`, never `timeout_ms = 10000`). A section keeps
//! each old key as a `Removed` field and declares it here with the release
//! that retired it, and [`crate::config::Config::resolve`] refuses every one
//! the sources set, naming its replacement. The next release deletes them
//! (the owner's one-release rule of 2026-11-09), which the test below holds:
//! it fails once the product version is past the release after `since`. No
//! specification governs this: our own design.

use ferrohealth_config::removed::Retired;

/// The release that renamed the keys below.
pub const SINCE: &str = "0.0.24";

/// `[audit]`.
pub mod audit {
    use super::{Retired, SINCE};

    /// `audit.spool_max_bytes`.
    pub const SPOOL_MAX_BYTES: Retired = Retired::replaced(
        "audit.spool_max_bytes",
        "audit.spool_max_size, a size such as `64MiB`",
        SINCE,
    );
    /// `audit.spool_write_timeout_ms`.
    pub const SPOOL_WRITE_TIMEOUT_MS: Retired = Retired::replaced(
        "audit.spool_write_timeout_ms",
        "audit.spool_write_timeout, a duration such as `2s`",
        SINCE,
    );
    /// `audit.timeout_ms`.
    pub const TIMEOUT_MS: Retired = Retired::replaced(
        "audit.timeout_ms",
        "audit.timeout, a duration such as `5s`",
        SINCE,
    );
    /// `audit.retry_max_ms`.
    pub const RETRY_MAX_MS: Retired = Retired::replaced(
        "audit.retry_max_ms",
        "audit.retry_max, a duration such as `1min`",
        SINCE,
    );
}

/// `[outbox]`.
pub mod outbox {
    use super::{Retired, SINCE};

    /// `outbox.retention_days`.
    pub const RETENTION_DAYS: Retired = Retired::replaced(
        "outbox.retention_days",
        "outbox.retention, a duration such as `30d`",
        SINCE,
    );
}

/// `[pmir.feed]`.
pub mod pmir {
    use super::{Retired, SINCE};

    /// `pmir.feed.initial_backoff_ms`.
    pub const INITIAL_BACKOFF_MS: Retired = Retired::replaced(
        "pmir.feed.initial_backoff_ms",
        "pmir.feed.initial_backoff, a duration such as `500ms`",
        SINCE,
    );
    /// `pmir.feed.max_backoff_ms`.
    pub const MAX_BACKOFF_MS: Retired = Retired::replaced(
        "pmir.feed.max_backoff_ms",
        "pmir.feed.max_backoff, a duration such as `1min`",
        SINCE,
    );
}

/// `[hl7v2]`, its consumers and its notification bounds.
pub mod hl7v2 {
    use super::{Retired, SINCE};

    /// `hl7v2.frame_limit_bytes`.
    pub const FRAME_LIMIT_BYTES: Retired = Retired::replaced(
        "hl7v2.frame_limit_bytes",
        "hl7v2.frame_limit, a size such as `1MiB`",
        SINCE,
    );
    /// `hl7v2.idle_timeout_ms`.
    pub const IDLE_TIMEOUT_MS: Retired = Retired::replaced(
        "hl7v2.idle_timeout_ms",
        "hl7v2.idle_timeout, a duration such as `5min`",
        SINCE,
    );
    /// `hl7v2.frame_timeout_ms`.
    pub const FRAME_TIMEOUT_MS: Retired = Retired::replaced(
        "hl7v2.frame_timeout_ms",
        "hl7v2.frame_timeout, a duration such as `30s`",
        SINCE,
    );
    /// `connect_timeout_ms` of a `[[hl7v2.consumers]]` entry.
    pub const CONNECT_TIMEOUT_MS: Retired = Retired::replaced(
        "connect_timeout_ms",
        "connect_timeout, a duration such as `5s`",
        SINCE,
    );
    /// `reply_timeout_ms` of a `[[hl7v2.consumers]]` entry.
    pub const REPLY_TIMEOUT_MS: Retired = Retired::replaced(
        "reply_timeout_ms",
        "reply_timeout, a duration such as `30s`",
        SINCE,
    );
    /// `hl7v2.notification.initial_backoff_ms`.
    pub const INITIAL_BACKOFF_MS: Retired = Retired::replaced(
        "hl7v2.notification.initial_backoff_ms",
        "hl7v2.notification.initial_backoff, a duration such as `1s`",
        SINCE,
    );
    /// `hl7v2.notification.max_backoff_ms`.
    pub const MAX_BACKOFF_MS: Retired = Retired::replaced(
        "hl7v2.notification.max_backoff_ms",
        "hl7v2.notification.max_backoff, a duration such as `5min`",
        SINCE,
    );
}

/// `[xcpd]`.
pub mod xcpd {
    use super::{Retired, SINCE};

    /// `xcpd.delivery_initial_backoff_ms`.
    pub const DELIVERY_INITIAL_BACKOFF_MS: Retired = Retired::replaced(
        "xcpd.delivery_initial_backoff_ms",
        "xcpd.delivery_initial_backoff, a duration such as `1s`",
        SINCE,
    );
    /// `xcpd.delivery_max_backoff_ms`.
    pub const DELIVERY_MAX_BACKOFF_MS: Retired = Retired::replaced(
        "xcpd.delivery_max_backoff_ms",
        "xcpd.delivery_max_backoff, a duration such as `1min`",
        SINCE,
    );
    /// `xcpd.delivery_timeout_ms`.
    pub const DELIVERY_TIMEOUT_MS: Retired = Retired::replaced(
        "xcpd.delivery_timeout_ms",
        "xcpd.delivery_timeout, a duration such as `10s`",
        SINCE,
    );
}

/// `[terminology]`.
pub mod terminology {
    use super::{Retired, SINCE};

    /// `terminology.timeout_ms`.
    pub const TIMEOUT_MS: Retired = Retired::replaced(
        "terminology.timeout_ms",
        "terminology.timeout, a duration such as `10s`",
        SINCE,
    );
    /// `terminology.retry.initial_backoff_ms`.
    pub const INITIAL_BACKOFF_MS: Retired = Retired::replaced(
        "terminology.retry.initial_backoff_ms",
        "terminology.retry.initial_backoff, a duration such as `200ms`",
        SINCE,
    );
    /// `terminology.retry.max_backoff_ms`.
    pub const MAX_BACKOFF_MS: Retired = Retired::replaced(
        "terminology.retry.max_backoff_ms",
        "terminology.retry.max_backoff, a duration such as `2s`",
        SINCE,
    );
}

/// Every key declared here.
pub const ALL: [Retired; 20] = [
    audit::SPOOL_MAX_BYTES,
    audit::SPOOL_WRITE_TIMEOUT_MS,
    audit::TIMEOUT_MS,
    audit::RETRY_MAX_MS,
    outbox::RETENTION_DAYS,
    pmir::INITIAL_BACKOFF_MS,
    pmir::MAX_BACKOFF_MS,
    hl7v2::FRAME_LIMIT_BYTES,
    hl7v2::IDLE_TIMEOUT_MS,
    hl7v2::FRAME_TIMEOUT_MS,
    hl7v2::CONNECT_TIMEOUT_MS,
    hl7v2::REPLY_TIMEOUT_MS,
    hl7v2::INITIAL_BACKOFF_MS,
    hl7v2::MAX_BACKOFF_MS,
    xcpd::DELIVERY_INITIAL_BACKOFF_MS,
    xcpd::DELIVERY_MAX_BACKOFF_MS,
    xcpd::DELIVERY_TIMEOUT_MS,
    terminology::TIMEOUT_MS,
    terminology::INITIAL_BACKOFF_MS,
    terminology::MAX_BACKOFF_MS,
];

#[cfg(test)]
mod tests {
    use ferrohealth_config::removed::stale;

    use super::{ALL, SINCE};

    /// The release a refusal declared in `since` is still alive in: `since`
    /// itself, and the version before it, which the release cut moves to it.
    fn alive(version: &str) -> bool {
        let next = version.rsplit_once('.').and_then(|(head, patch)| {
            Some(format!(
                "{head}.{}",
                patch.parse::<u32>().ok()?.checked_add(1)?
            ))
        });
        version == SINCE || next.as_deref() == Some(SINCE)
    }

    #[test]
    fn every_refusal_lives_for_the_one_release_that_renamed_its_key() {
        let version = env!("CARGO_PKG_VERSION");
        assert!(
            alive(version),
            "v{SINCE} renamed these keys, and v{version} is neither that release nor the one before it: past it, delete the refusals of src/config/retired.rs"
        );
        assert!(
            stale(&ALL, SINCE).is_empty(),
            "every refusal records v{SINCE}"
        );
    }
}
