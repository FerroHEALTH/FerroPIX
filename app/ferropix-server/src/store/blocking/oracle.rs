// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What a blocking read must find, for the tests: the reads over every
//! version that the keys of the head replaced (#724), and the definition
//! of a block, each current head whose demographics give the key, with the
//! scoring features those demographics give (#747).

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgPool;
use uuid::Uuid;

use crate::config::section::matching::BlockingKey;
use crate::matching::blocking::text_key;
use crate::matching::features::{self, FEATURES_VERSION};
use crate::matching::scorer::Scorer;
use crate::matching::subject::Subject;
use crate::pdqm::matching::{Best, FEW_FIELDS, Matched};
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

/// A row of the feed's read: record, head version, person, scoring
/// features.
pub(super) type FeedRow = (Uuid, i32, Uuid, String);

/// A row of ITI-119's read: record, person, scoring features.
pub(super) type MatchRow = (Uuid, Uuid, String);

/// A row of the feed's read as the store answers it: record, head version,
/// person, scoring features, their version.
pub(super) type StoredFeedRow = (Uuid, i32, Uuid, Option<String>, Option<i16>);

/// A row of ITI-119's read as the store answers it: record, person, scoring
/// features, their version.
pub(super) type StoredMatchRow = (Uuid, Uuid, Option<String>, Option<i16>);

/// The subject `demographics` give, without local identifiers.
fn subject(demographics: &str) -> Subject {
    let patient = demographics::patient(demographics).expect("stored demographics read");
    Subject::from_patient(&patient, BTreeSet::new())
}

/// The two text keys `demographics` give: phonetic, postal.
pub(super) fn keys(demographics: &str) -> (Option<String>, Option<String>) {
    let subject = subject(demographics);
    (
        text_key(BlockingKey::BirthYearFamilyPhonetic, &subject),
        text_key(BlockingKey::PostalCodeBirthDate, &subject),
    )
}

/// The scoring features `demographics` give, through the `Patient`.
pub(super) fn features_of(demographics: &str) -> String {
    features::encode(&subject(demographics)).expect("the features are written")
}

/// The features of a stored row, which must be of [`FEATURES_VERSION`] and
/// read back as a subject.
fn stored(features: Option<String>, version: Option<i16>) -> String {
    assert_eq!(
        Some(FEATURES_VERSION),
        version,
        "the head's features are current"
    );
    let features = features.expect("the head holds features");
    features::decode(Some(&features), version, BTreeSet::new()).expect("the features read");
    features
}

/// The feed rows of an old read, whose last column is demographics, with
/// the features those give.
pub(super) fn old_feed_rows(rows: Vec<FeedRow>) -> Vec<FeedRow> {
    rows.into_iter()
        .map(|(id, version, person, text)| (id, version, person, features_of(&text)))
        .collect()
}

/// The match rows of an old read, with the features its demographics give.
pub(super) fn old_match_rows(rows: Vec<MatchRow>) -> Vec<MatchRow> {
    rows.into_iter()
        .map(|(id, person, text)| (id, person, features_of(&text)))
        .collect()
}

/// The feed rows of a read over the head, its features checked current.
pub(super) fn current_feed_rows(rows: Vec<StoredFeedRow>) -> Vec<FeedRow> {
    rows.into_iter()
        .map(|(id, head, person, features, version)| (id, head, person, stored(features, version)))
        .collect()
}

/// The match rows of a read over the head, its features checked current.
pub(super) fn current_match_rows(rows: Vec<StoredMatchRow>) -> Vec<MatchRow> {
    rows.into_iter()
        .map(|(id, person, features, version)| (id, person, stored(features, version)))
        .collect()
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
                row: (id, version, person, features_of(&text)),
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

/// Asserts that every record, current or not, holds the keys and the
/// scoring features its head version's demographics give, of
/// [`FEATURES_VERSION`].
pub(super) async fn assert_heads_hold_their_keys(pool: &PgPool) {
    type Held = (Option<String>, Option<String>, Option<String>, Option<i16>);
    type Row = (
        Uuid,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i16>,
        String,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT r.id, b.block_phonetic, b.block_postal, b.features, b.features_version,
                    v.demographics::text
             FROM pix.source_record r
             LEFT JOIN pix.record_block b ON b.record_id = r.id
             JOIN pix.source_record_version v
                  ON v.record_id = r.id AND v.version = r.head_version",
    )
    .fetch_all(pool)
    .await
    .expect("the records read");
    let wrong: BTreeMap<Uuid, Held> = rows
        .into_iter()
        .filter_map(|(id, phonetic, postal, features, version, text)| {
            let (want_phonetic, want_postal) = keys(&text);
            let want: Held = (
                want_phonetic,
                want_postal,
                Some(features_of(&text)),
                Some(FEATURES_VERSION),
            );
            let held: Held = (phonetic, postal, features, version);
            (want != held).then_some((id, held))
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} records hold keys or features their head does not give",
        wrong.len()
    );
}

/// One current head as ITI-119 scores it: the subject its demographics
/// give through the `Patient`, and the one its features give.
#[derive(Debug, Clone)]
pub(super) struct Scorable {
    record: Uuid,
    person: Uuid,
    unretired: bool,
    phonetic: Option<String>,
    postal: Option<String>,
    /// Through the `Patient`, the path the features replaced.
    pub(super) given: Subject,
    /// From the head's features.
    pub(super) kept: Subject,
}

/// Every current head, scorable both ways.
pub(super) async fn scorable(pool: &PgPool) -> Vec<Scorable> {
    type Row = (Uuid, Uuid, String, bool, Option<String>, Option<i16>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT r.id, l.person_id, v.demographics::text, d.retired_at IS NULL,
                b.features, b.features_version
         FROM pix.source_record r
         JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
         JOIN pix.identifier_domain d ON d.id = r.domain_id
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
         JOIN pix.record_block b ON b.record_id = r.id
         WHERE r.active AND r.removed_at IS NULL",
    )
    .fetch_all(pool)
    .await
    .expect("the heads read");
    rows.into_iter()
        .map(|(record, person, text, unretired, kept, version)| {
            let (phonetic, postal) = keys(&text);
            Scorable {
                record,
                person,
                unretired,
                phonetic,
                postal,
                given: subject(&text),
                kept: features::decode(kept.as_deref(), version, BTreeSet::new())
                    .expect("the features read"),
            }
        })
        .collect()
}

/// The heads ITI-119 scores for `input` over the text keys `keys`, each
/// with the first key that finds it, in record order.
pub(super) fn blocked<'h>(
    heads: &'h [Scorable],
    keys: &[BlockingKey],
    input: &Subject,
) -> Vec<(&'h Scorable, BlockingKey)> {
    let mut found: BTreeMap<Uuid, (&Scorable, BlockingKey)> = BTreeMap::new();
    for key in keys {
        let Some(value) = text_key(*key, input) else {
            continue;
        };
        for head in heads.iter().filter(|head| head.unretired) {
            let held = if *key == BlockingKey::BirthYearFamilyPhonetic {
                head.phonetic.as_deref()
            } else {
                head.postal.as_deref()
            };
            if held == Some(value.as_str()) {
                found.entry(head.record).or_insert((head, *key));
            }
        }
    }
    found.into_values().collect()
}

/// What ITI-119 answers for `input` when each candidate is read through its
/// `Patient`.
pub(super) fn matched_persons(
    heads: &[Scorable],
    scorer: &Scorer,
    keys: &[BlockingKey],
    input: &Subject,
) -> Matched {
    let mut best = Best::default();
    for (head, key) in blocked(heads, keys, input) {
        best.keep(head.person, &scorer.score(input, &head.given, Some(key)));
    }
    Matched {
        found: best.found(),
        few_fields: input.stated() < FEW_FIELDS,
    }
}
