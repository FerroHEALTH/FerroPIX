// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The keys of FerroPIX's own sections that v0.0.23 renamed, each refused by
//! name for that one release.
//!
//! v0.0.23 writes every span of time and size as `ferrohealth-config`'s
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
pub const SINCE: &str = "0.0.23";

/// A key `key` replaced by `replacement`, with the form of its value.
const fn renamed(key: &'static str, replacement: &'static str) -> Retired {
    Retired::replaced(key, replacement, SINCE)
}

/// `[audit]`.
pub mod audit {
    use super::{Retired, renamed};

    /// `audit.spool_max_bytes`.
    pub const SPOOL_MAX_BYTES: Retired = renamed(
        "audit.spool_max_bytes",
        "audit.spool_max_size, a size such as `64MiB`",
    );
    /// `audit.spool_write_timeout_ms`.
    pub const SPOOL_WRITE_TIMEOUT_MS: Retired = renamed(
        "audit.spool_write_timeout_ms",
        "audit.spool_write_timeout, a duration such as `2s`",
    );
    /// `audit.timeout_ms`.
    pub const TIMEOUT_MS: Retired =
        renamed("audit.timeout_ms", "audit.timeout, a duration such as `5s`");
    /// `audit.retry_max_ms`.
    pub const RETRY_MAX_MS: Retired = renamed(
        "audit.retry_max_ms",
        "audit.retry_max, a duration such as `1min`",
    );
}

/// `[outbox]`.
pub mod outbox {
    use super::{Retired, renamed};

    /// `outbox.retention_days`.
    pub const RETENTION_DAYS: Retired = renamed(
        "outbox.retention_days",
        "outbox.retention, a duration such as `30d`",
    );
}

/// `[pmir.feed]`.
pub mod pmir {
    use super::{Retired, renamed};

    /// `pmir.feed.initial_backoff_ms`.
    pub const INITIAL_BACKOFF_MS: Retired = renamed(
        "pmir.feed.initial_backoff_ms",
        "pmir.feed.initial_backoff, a duration such as `500ms`",
    );
    /// `pmir.feed.max_backoff_ms`.
    pub const MAX_BACKOFF_MS: Retired = renamed(
        "pmir.feed.max_backoff_ms",
        "pmir.feed.max_backoff, a duration such as `1min`",
    );
}

/// `[hl7v2]`, its consumers and its notification bounds.
pub mod hl7v2 {
    use super::{Retired, renamed};

    /// `hl7v2.frame_limit_bytes`.
    pub const FRAME_LIMIT_BYTES: Retired = renamed(
        "hl7v2.frame_limit_bytes",
        "hl7v2.frame_limit, a size such as `1MiB`",
    );
    /// `hl7v2.idle_timeout_ms`.
    pub const IDLE_TIMEOUT_MS: Retired = renamed(
        "hl7v2.idle_timeout_ms",
        "hl7v2.idle_timeout, a duration such as `5min`",
    );
    /// `hl7v2.frame_timeout_ms`.
    pub const FRAME_TIMEOUT_MS: Retired = renamed(
        "hl7v2.frame_timeout_ms",
        "hl7v2.frame_timeout, a duration such as `30s`",
    );
    /// `connect_timeout_ms` of a `[[hl7v2.consumers]]` entry.
    pub const CONNECT_TIMEOUT_MS: Retired = renamed(
        "connect_timeout_ms",
        "connect_timeout, a duration such as `5s`",
    );
    /// `reply_timeout_ms` of a `[[hl7v2.consumers]]` entry.
    pub const REPLY_TIMEOUT_MS: Retired = renamed(
        "reply_timeout_ms",
        "reply_timeout, a duration such as `30s`",
    );
    /// `hl7v2.notification.initial_backoff_ms`.
    pub const INITIAL_BACKOFF_MS: Retired = renamed(
        "hl7v2.notification.initial_backoff_ms",
        "hl7v2.notification.initial_backoff, a duration such as `1s`",
    );
    /// `hl7v2.notification.max_backoff_ms`.
    pub const MAX_BACKOFF_MS: Retired = renamed(
        "hl7v2.notification.max_backoff_ms",
        "hl7v2.notification.max_backoff, a duration such as `5min`",
    );
}

/// `[xcpd]`.
pub mod xcpd {
    use super::{Retired, renamed};

    /// `xcpd.delivery_initial_backoff_ms`.
    pub const DELIVERY_INITIAL_BACKOFF_MS: Retired = renamed(
        "xcpd.delivery_initial_backoff_ms",
        "xcpd.delivery_initial_backoff, a duration such as `1s`",
    );
    /// `xcpd.delivery_max_backoff_ms`.
    pub const DELIVERY_MAX_BACKOFF_MS: Retired = renamed(
        "xcpd.delivery_max_backoff_ms",
        "xcpd.delivery_max_backoff, a duration such as `1min`",
    );
    /// `xcpd.delivery_timeout_ms`.
    pub const DELIVERY_TIMEOUT_MS: Retired = renamed(
        "xcpd.delivery_timeout_ms",
        "xcpd.delivery_timeout, a duration such as `10s`",
    );
}

/// `[terminology]`.
pub mod terminology {
    use super::{Retired, renamed};

    /// `terminology.timeout_ms`.
    pub const TIMEOUT_MS: Retired = renamed(
        "terminology.timeout_ms",
        "terminology.timeout, a duration such as `10s`",
    );
    /// `terminology.retry.initial_backoff_ms`.
    pub const INITIAL_BACKOFF_MS: Retired = renamed(
        "terminology.retry.initial_backoff_ms",
        "terminology.retry.initial_backoff, a duration such as `200ms`",
    );
    /// `terminology.retry.max_backoff_ms`.
    pub const MAX_BACKOFF_MS: Retired = renamed(
        "terminology.retry.max_backoff_ms",
        "terminology.retry.max_backoff, a duration such as `2s`",
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
