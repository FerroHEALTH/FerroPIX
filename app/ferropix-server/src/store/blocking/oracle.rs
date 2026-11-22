// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What a blocking read must find, for the tests: the reads over every
//! version that the keys of the head replaced (#724), and the definition
//! of a block, each current head whose demographics give the key.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgPool;
use uuid::Uuid;

use crate::config::section::matching::BlockingKey;
use crate::matching::blocking::text_key;
use crate::matching::subject::Subject;
use crate::store::demographics;

/// The feed's read by the version column `$column` before the keys moved
/// to the head, verbatim.
macro_rules! old_feed_read {
    ($column:literal) => {
        concat!(
            "SELECT r.id, r.head_version, l.person_id, v.demographics::text
             FROM pix.source_record_version v
             JOIN pix.source_record r ON r.id = v.record_id AND v.version = r.head_version
             JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
             WHERE v.",
            $column,
            " = $1 AND r.id <> $2 AND l.person_id <> $3
               AND r.active AND r.removed_at IS NULL"
        )
    };
}

/// ITI-119's read by the version column `$column` before the keys moved to
/// the head, verbatim.
macro_rules! old_match_read {
    ($column:literal) => {
        concat!(
            "SELECT r.id, l.person_id, v.demographics::text
             FROM pix.source_record_version v
             JOIN pix.source_record r ON r.id = v.record_id AND v.version = r.head_version
             JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
             JOIN pix.identifier_domain d ON d.id = r.domain_id
             WHERE v.",
            $column,
            " = $1
               AND r.active AND r.removed_at IS NULL AND d.retired_at IS NULL"
        )
    };
}

/// The feed's old phonetic read.
pub(super) const OLD_FEED_BY_PHONETIC: &str = old_feed_read!("block_phonetic");

/// The feed's old postal read.
pub(super) const OLD_FEED_BY_POSTAL: &str = old_feed_read!("block_postal");

/// ITI-119's old phonetic read.
pub(super) const OLD_MATCH_BY_PHONETIC: &str = old_match_read!("block_phonetic");

/// ITI-119's old postal read.
pub(super) const OLD_MATCH_BY_POSTAL: &str = old_match_read!("block_postal");

/// A row of the feed's read: record, head version, person, demographics.
pub(super) type FeedRow = (Uuid, i32, Uuid, String);

/// A row of ITI-119's read: record, person, demographics.
pub(super) type MatchRow = (Uuid, Uuid, String);

/// The two text keys `demographics` give: phonetic, postal.
pub(super) fn keys(demographics: &str) -> (Option<String>, Option<String>) {
    let patient = demographics::patient(demographics).expect("stored demographics read");
    let subject = Subject::from_patient(&patient, BTreeSet::new());
    (
        text_key(BlockingKey::BirthYearFamilyPhonetic, &subject),
        text_key(BlockingKey::PostalCodeBirthDate, &subject),
    )
}

/// One current head: a record that is active, unremoved and linked.
#[derive(Debug, Clone)]
pub(super) struct Head {
    row: FeedRow,
    unretired: bool,
    phonetic: Option<String>,
    postal: Option<String>,
}

/// Every current head, with the keys its demographics give.
pub(super) async fn heads(pool: &PgPool) -> Vec<Head> {
    let rows: Vec<(Uuid, i32, Uuid, String, bool)> = sqlx::query_as(
        "SELECT r.id, r.head_version, l.person_id, v.demographics::text, d.retired_at IS NULL
         FROM pix.source_record r
         JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
         JOIN pix.identifier_domain d ON d.id = r.domain_id
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
         WHERE r.active AND r.removed_at IS NULL",
    )
    .fetch_all(pool)
    .await
    .expect("the heads read");
    rows.into_iter()
        .map(|(id, version, person, text, unretired)| {
            let (phonetic, postal) = keys(&text);
            Head {
                row: (id, version, person, text),
                unretired,
                phonetic,
                postal,
            }
        })
        .collect()
}

/// The head's key of the kind `phonetic` names.
fn key(head: &Head, phonetic: bool) -> Option<&str> {
    if phonetic {
        head.phonetic.as_deref()
    } else {
        head.postal.as_deref()
    }
}

/// What the feed's read must find for `value`, excluding `record` and
/// `person`.
pub(super) fn feed(
    heads: &[Head],
    phonetic: bool,
    value: &str,
    record: Uuid,
    person: Uuid,
) -> Vec<FeedRow> {
    let mut found: Vec<FeedRow> = heads
        .iter()
        .filter(|head| key(head, phonetic) == Some(value))
        .filter(|head| head.row.0 != record && head.row.2 != person)
        .map(|head| head.row.clone())
        .collect();
    found.sort();
    found
}

/// What ITI-119's read must find for `value`.
pub(super) fn matched(heads: &[Head], phonetic: bool, value: &str) -> Vec<MatchRow> {
    let mut found: Vec<MatchRow> = heads
        .iter()
        .filter(|head| head.unretired && key(head, phonetic) == Some(value))
        .map(|head| (head.row.0, head.row.2, head.row.3.clone()))
        .collect();
    found.sort();
    found
}

/// Asserts that every record, current or not, holds the keys its head
/// version's demographics give.
pub(super) async fn assert_heads_hold_their_keys(pool: &PgPool) {
    let rows: Vec<(Uuid, Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT r.id, b.block_phonetic, b.block_postal, v.demographics::text
         FROM pix.source_record r
         LEFT JOIN pix.record_block b ON b.record_id = r.id
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version",
    )
    .fetch_all(pool)
    .await
    .expect("the records read");
    let wrong: BTreeMap<Uuid, (Option<String>, Option<String>)> = rows
        .into_iter()
        .filter_map(|(id, phonetic, postal, text)| {
            let given = (phonetic, postal);
            (keys(&text) != given).then_some((id, given))
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} records hold keys their head does not give",
        wrong.len()
    );
}
