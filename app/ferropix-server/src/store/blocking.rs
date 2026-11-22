// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The blocking reads: the current records that carry a text blocking key.
//!
//! The two text keys of `docs/architecture.md` §5.2 live in `record_block`,
//! one row per record written by the statement that moves its head, so each
//! read is one probe of the key's index and touches only the heads that
//! carry the key, whatever the history behind them. The same row holds the
//! head's scoring features ([`crate::matching::features`]), so no read
//! reaches a version's demographics. The feed weighs a version against
//! records in other persons; an ITI-119 match reads every record of an
//! unretired domain. No specification governs the blocking: our own design.

/// The feed's read of the heads whose key `$column` is `$1`, in another
/// person than `$3` and other than `$2`: id, head version, person, scoring
/// features and their version. Each record the key's index finds is read
/// through its own keys, at most one row each (one record, one open link).
macro_rules! feed_read {
    ($column:literal) => {
        concat!(
            "SELECT f.id, f.head_version, f.person_id, b.features, b.features_version
             FROM pix.record_block b
             CROSS JOIN LATERAL (
                 SELECT r.id, r.head_version, l.person_id
                 FROM pix.source_record r
                 JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
                 WHERE r.id = b.record_id AND l.person_id <> $3
                   AND r.active AND r.removed_at IS NULL
                 LIMIT 1
             ) AS f
             WHERE b.",
            $column,
            " = $1 AND b.record_id <> $2"
        )
    };
}

/// ITI-119's read of the heads of unretired domains whose key `$column` is
/// `$1`: id, person, scoring features and their version, each record read as
/// the feed's read reads it.
macro_rules! match_read {
    ($column:literal) => {
        concat!(
            "SELECT f.id, f.person_id, b.features, b.features_version
             FROM pix.record_block b
             CROSS JOIN LATERAL (
                 SELECT r.id, l.person_id
                 FROM pix.source_record r
                 JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
                 JOIN pix.identifier_domain d ON d.id = r.domain_id
                 WHERE r.id = b.record_id
                   AND r.active AND r.removed_at IS NULL AND d.retired_at IS NULL
                 LIMIT 1
             ) AS f
             WHERE b.",
            $column,
            " = $1"
        )
    };
}

/// The feed's read by the phonetic key.
pub const FEED_BY_PHONETIC: &str = feed_read!("block_phonetic");

/// The feed's read by the postal key.
pub const FEED_BY_POSTAL: &str = feed_read!("block_postal");

/// ITI-119's read by the phonetic key.
pub const MATCH_BY_PHONETIC: &str = match_read!("block_phonetic");

/// ITI-119's read by the postal key.
pub const MATCH_BY_POSTAL: &str = match_read!("block_postal");

#[cfg(test)]
mod e2e;
#[cfg(test)]
mod oracle;
