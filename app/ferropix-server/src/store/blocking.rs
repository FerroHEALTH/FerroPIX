// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The blocking reads: the current records that carry a text blocking key.
//!
//! The two text keys of `docs/architecture.md` §5.2 live on the record,
//! written by the statement that moves its head, so each read is one probe
//! of the key's index on `source_record` and touches only the heads that
//! carry the key, whatever the history behind them. The head version's
//! demographics are read through its `(record_id, version)` key. The feed
//! weighs a version against records in other persons; an ITI-119 match
//! reads every record of an unretired domain. No specification governs the
//! blocking: our own design.

/// The records in another person than `$3`, other than `$2`, whose head
/// has the phonetic key `$1`: id, head version, person, demographics.
pub const FEED_BY_PHONETIC: &str = "SELECT r.id, r.head_version, l.person_id, v.demographics::text
     FROM pix.source_record r
     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
     CROSS JOIN LATERAL (
         SELECT h.demographics FROM pix.source_record_version h
         WHERE h.record_id = r.id AND h.version = r.head_version
     ) AS v
     WHERE r.block_phonetic = $1 AND r.id <> $2 AND l.person_id <> $3
       AND r.active AND r.removed_at IS NULL";

/// The records in another person than `$3`, other than `$2`, whose head
/// has the postal key `$1`: id, head version, person, demographics.
pub const FEED_BY_POSTAL: &str = "SELECT r.id, r.head_version, l.person_id, v.demographics::text
     FROM pix.source_record r
     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
     CROSS JOIN LATERAL (
         SELECT h.demographics FROM pix.source_record_version h
         WHERE h.record_id = r.id AND h.version = r.head_version
     ) AS v
     WHERE r.block_postal = $1 AND r.id <> $2 AND l.person_id <> $3
       AND r.active AND r.removed_at IS NULL";

/// The records of unretired domains whose head has the phonetic key `$1`:
/// id, person, demographics.
pub const MATCH_BY_PHONETIC: &str = "SELECT r.id, l.person_id, v.demographics::text
     FROM pix.source_record r
     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
     JOIN pix.identifier_domain d ON d.id = r.domain_id
     CROSS JOIN LATERAL (
         SELECT h.demographics FROM pix.source_record_version h
         WHERE h.record_id = r.id AND h.version = r.head_version
     ) AS v
     WHERE r.block_phonetic = $1
       AND r.active AND r.removed_at IS NULL AND d.retired_at IS NULL";

/// The records of unretired domains whose head has the postal key `$1`:
/// id, person, demographics.
pub const MATCH_BY_POSTAL: &str = "SELECT r.id, l.person_id, v.demographics::text
     FROM pix.source_record r
     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
     JOIN pix.identifier_domain d ON d.id = r.domain_id
     CROSS JOIN LATERAL (
         SELECT h.demographics FROM pix.source_record_version h
         WHERE h.record_id = r.id AND h.version = r.head_version
     ) AS v
     WHERE r.block_postal = $1
       AND r.active AND r.removed_at IS NULL AND d.retired_at IS NULL";

#[cfg(test)]
mod e2e;
#[cfg(test)]
mod oracle;
