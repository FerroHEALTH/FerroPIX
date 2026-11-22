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
//! unretired domain. Each read also carries the identifiers the head of
//! each record it finds holds in `local` domains, which the scorer
//! compares, read in the same statement by one probe over all of them. No
//! specification governs the blocking: our own design.

use std::collections::BTreeSet;

use uuid::Uuid;

use crate::matching::subject::IdentifierKey;
use crate::store::seal::SealError;

/// The read `$inner` of records (with `id` and `head_version` among its
/// columns) answered as `$columns` of each record beside the identifiers
/// its head carries in `local` domains, each as its domain's 16 bytes and
/// its 32-byte HMAC in one `bytea` (null for a record with none), then
/// `$tail`. The identifiers are read by one index scan
/// over every record found, as `= ANY` of an array.
macro_rules! with_local_keys {
    ($inner:expr, $columns:literal, $tail:literal) => {
        concat!(
            "WITH f AS MATERIALIZED (",
            $inner,
            "), k AS (
                 SELECT i.record_id,
                        string_agg(uuid_send(i.domain_id) || i.identifier_hmac, ''::bytea) AS keys
                 FROM pix.carried_identifier i
                 JOIN f ON f.id = i.record_id AND f.head_version = i.version
                 JOIN pix.identifier_domain d ON d.id = i.domain_id AND d.kind = 'local'
                 WHERE i.record_id = ANY (ARRAY(SELECT f.id FROM f))
                 GROUP BY i.record_id
             )
             SELECT ",
            $columns,
            ", k.keys
             FROM f LEFT JOIN k ON k.record_id = f.id ",
            $tail
        )
    };
}

/// The feed's read of the heads whose key `$column` is `$1`, in another
/// person than `$3` and other than `$2`: id, head version, person, scoring
/// features and their version. Each record the key's index finds is read
/// through its own keys, at most one row each (one record, one open link).
macro_rules! feed_read {
    ($column:literal) => {
        with_local_keys!(
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
            ),
            "f.id, f.head_version, f.person_id, f.features, f.features_version",
            ""
        )
    };
}

/// ITI-119's read of the heads of unretired domains whose key `$column` is
/// `$1`: id, person, scoring features and their version, each record read as
/// the feed's read reads it.
macro_rules! match_read {
    ($column:literal) => {
        with_local_keys!(
            concat!(
                "SELECT f.id, f.head_version, f.person_id, b.features, b.features_version
             FROM pix.record_block b
             CROSS JOIN LATERAL (
                 SELECT r.id, r.head_version, l.person_id
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
            ),
            "f.id, f.person_id, f.features, f.features_version",
            ""
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

/// The feed's read of the current records, other than `$3`, whose head
/// carries one of the identifiers `($1, $2)`, as [`FEED_BY_PHONETIC`] reads
/// them, ordered by id.
pub const FEED_BY_IDENTIFIER: &str = with_local_keys!(
    "SELECT DISTINCT r.id, r.head_version, l.person_id, b.features, b.features_version
     FROM pix.carried_identifier c
     JOIN pix.source_record r ON r.id = c.record_id AND c.version = r.head_version
     JOIN pix.record_block b ON b.record_id = r.id
     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
     WHERE (c.domain_id, c.identifier_hmac) IN (SELECT * FROM unnest($1::uuid[], $2::bytea[]))
       AND r.id <> $3 AND r.active AND r.removed_at IS NULL",
    "f.id, f.head_version, f.person_id, f.features, f.features_version",
    "ORDER BY f.id"
);

/// The feed's read of the records in another person than `$4`, other than
/// `$3`, whose head carries one of the local identifiers `($1, $2)`.
pub const FEED_BY_SHARED: &str = with_local_keys!(
    "SELECT DISTINCT r.id, r.head_version, l.person_id, b.features, b.features_version
     FROM pix.carried_identifier c
     JOIN pix.source_record r ON r.id = c.record_id AND c.version = r.head_version
     JOIN pix.record_block b ON b.record_id = r.id
     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
     WHERE (c.domain_id, c.identifier_hmac) IN (SELECT * FROM unnest($1::uuid[], $2::bytea[]))
       AND r.id <> $3 AND l.person_id <> $4 AND r.active AND r.removed_at IS NULL",
    "f.id, f.head_version, f.person_id, f.features, f.features_version",
    ""
);

/// ITI-119's read of the records of unretired domains whose head carries one
/// of the local identifiers `($1, $2)`.
pub const MATCH_BY_SHARED: &str = with_local_keys!(
    "SELECT DISTINCT r.id, r.head_version, l.person_id, b.features, b.features_version
     FROM pix.carried_identifier c
     JOIN pix.source_record r ON r.id = c.record_id AND c.version = r.head_version
     JOIN pix.record_block b ON b.record_id = r.id
     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
     JOIN pix.identifier_domain rd ON rd.id = r.domain_id
     WHERE (c.domain_id, c.identifier_hmac) IN (SELECT * FROM unnest($1::uuid[], $2::bytea[]))
       AND r.active AND r.removed_at IS NULL AND rd.retired_at IS NULL",
    "f.id, f.person_id, f.features, f.features_version",
    ""
);

/// The local identifier keys of one row of a read, from its `bytea` of
/// 48-byte keys, null for a record with none.
///
/// # Errors
///
/// Returns [`SealError::Malformed`] when the bytes are not whole keys.
pub fn carried_keys(keys: Option<&[u8]>) -> Result<BTreeSet<IdentifierKey>, SealError> {
    // A record with no local identifiers has no bytes.
    let keys = keys.unwrap_or_default();
    let (whole, rest) = keys.as_chunks::<48>();
    if !rest.is_empty() {
        return Err(SealError::Malformed);
    }
    whole
        .iter()
        .map(|key| {
            let (domain, hmac) = key.split_first_chunk::<16>().ok_or(SealError::Malformed)?;
            let hmac: [u8; 32] = (*hmac).try_into().map_err(|_short| SealError::Malformed)?;
            Ok(IdentifierKey::new(Uuid::from_bytes(*domain), hmac))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::carried_keys;
    use crate::matching::subject::IdentifierKey;
    use crate::store::seal::SealError;
    use uuid::Uuid;

    #[test]
    fn the_keys_read_back_from_their_bytes() {
        let first = (Uuid::from_u128(7), [3_u8; 32]);
        let second = (Uuid::from_u128(9), [5_u8; 32]);
        let mut bytes = Vec::new();
        for (domain, hmac) in [first, second] {
            bytes.extend_from_slice(domain.as_bytes());
            bytes.extend_from_slice(&hmac);
        }
        let keys = carried_keys(Some(&bytes)).expect("whole keys");
        assert_eq!(
            vec![
                IdentifierKey::new(first.0, first.1),
                IdentifierKey::new(second.0, second.1)
            ],
            keys.into_iter().collect::<Vec<_>>()
        );
        assert!(carried_keys(None).expect("none").is_empty());
    }

    #[test]
    fn bytes_that_are_not_whole_keys_are_refused() {
        assert!(matches!(
            carried_keys(Some(&[0_u8; 47])),
            Err(SealError::Malformed)
        ));
    }
}
