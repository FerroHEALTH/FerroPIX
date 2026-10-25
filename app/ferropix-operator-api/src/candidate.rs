// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The review queue and one candidate with its evidence,
//! `GET {base}/operator/candidates` and `GET {base}/operator/candidates/{id}`,
//! and the steward's decision, `POST {base}/operator/candidates/{id}/decide`.
//!
//! A candidate is a pair of source records the scorer found worth a human
//! look. The queue lists the open candidates and the deferred ones whose
//! date has come, the grade first, then the score, then the age. A decision
//! carries the versions the steward read, so a decision made on a stale
//! screen is refused with `409`. No specification governs the queue: our
//! own design.

use serde::{Deserialize, Serialize};

use crate::patient::{Demographics, DomainRef, Grade, Masked, PersonStatus};

/// The most candidates one page of the queue lists.
pub const PAGE: u32 = 50;

/// Where a candidate stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Waiting for a decision.
    Open,
    /// Decided: the records are one patient.
    Linked,
    /// Decided: the records are two patients, until either changes.
    NotAMatch,
    /// Put off until a date.
    Deferred,
}

/// The filters of the queue, the query string of
/// `GET {base}/operator/candidates`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateQuery {
    /// Only candidates of this grade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<Grade>,
    /// Only candidates with a record in the domain of this system.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// Only candidates queued at least this many days ago.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub older_than_days: Option<u32>,
    /// List the deferred candidates whose date has not come, in place of
    /// the queue.
    #[serde(default)]
    pub deferred: bool,
    /// The candidates skipped before this page.
    #[serde(default)]
    pub offset: u32,
}

/// One side of a candidate in the queue: what the row shows of a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Side {
    /// The record's domain.
    pub domain: DomainRef,
    /// The leading name, family name and given names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The birth date as FHIR writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birth_date: Option<String>,
}

/// One row of the queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateRow {
    /// The candidate's id.
    pub id: String,
    /// The first record.
    pub a: Side,
    /// The second record.
    pub b: Side,
    /// The score, in `[0, 1]`.
    pub score: f64,
    /// The grade of the score.
    pub grade: Grade,
    /// Where the candidate stands.
    pub status: Status,
    /// When the candidate was queued, RFC 3339 UTC.
    pub created_at: String,
    /// The date a deferred candidate comes back, RFC 3339 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deferred_until: Option<String>,
}

/// What `GET {base}/operator/candidates` answers with: one page of the
/// queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidates {
    /// Every candidate the filters admit.
    pub total: u64,
    /// The candidates skipped before this page.
    pub offset: u32,
    /// The offset of the next page, when candidates follow this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<u32>,
    /// The candidates of this page, in the queue's order.
    pub rows: Vec<CandidateRow>,
    /// When the last decision was made, RFC 3339 UTC, for the empty queue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_decision_at: Option<String>,
}

/// How one field compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Agreement {
    /// Absent on one side or both: no weight.
    Absent,
    /// The field differs.
    Disagree,
    /// The field agrees in part.
    Partial,
    /// The field agrees.
    Agree,
}

/// The evidence of one field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldEvidence {
    /// The field, as `[matching.fields]` names it.
    pub field: String,
    /// The comparator it was read with.
    pub comparator: String,
    /// The first record's value as compared, masked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<String>,
    /// The second record's value as compared, masked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<String>,
    /// How the field compared.
    pub agreement: Agreement,
    /// The comparator's reading, where it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading: Option<f64>,
    /// The weight the field added, in bits.
    pub weight: f64,
}

/// Why a pair scored as it did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    /// The hard conflicts that held a shared authoritative identifier from
    /// linking the pair (`birth_year`, `gender`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
    /// The blocking key that produced the pair, as `[matching]` names it;
    /// none for a pair a shared authoritative identifier named.
    #[serde(default)]
    pub blocking_key: Option<String>,
    /// The sum of the weights, in bits.
    pub total: f64,
    /// The score.
    pub score: f64,
    /// The grade.
    pub grade: Grade,
    /// One entry per field the scorer compares.
    pub fields: Vec<FieldEvidence>,
}

/// One identifier of a candidate record's person, masked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonIdentifier {
    /// The identifier's domain.
    pub domain: DomainRef,
    /// The identifier, masked.
    pub masked: Masked,
}

/// One record of a candidate, as the compare draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateRecord {
    /// The record's id.
    pub id: String,
    /// The version the candidate was computed on, which a decision carries.
    pub version: i32,
    /// The record's current version.
    pub head_version: i32,
    /// The record's domain.
    pub domain: DomainRef,
    /// The record's demographics at the candidate's version.
    pub demographics: Demographics,
    /// The person the record is in.
    pub person: String,
    /// Whether that person is active or merged.
    pub person_status: PersonStatus,
    /// Every identifier of that person, masked.
    pub identifiers: Vec<PersonIdentifier>,
}

/// The `[matching]` thresholds, drawn on the score's scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thresholds {
    /// The score at or above which a pair grades certain, `link_at`.
    pub certain: f64,
    /// The score at or above which a pair grades probable, `review_at`.
    pub probable: f64,
    /// The score at or above which a pair grades possible, `possible_at`.
    pub possible: f64,
}

/// What `GET {base}/operator/candidates/{id}` answers with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    /// The candidate's id.
    pub id: String,
    /// Where the candidate stands.
    pub status: Status,
    /// The score, in `[0, 1]`.
    pub score: f64,
    /// The grade of the score.
    pub grade: Grade,
    /// Why the pair scored as it did.
    pub evidence: Evidence,
    /// The first record.
    pub a: CandidateRecord,
    /// The second record.
    pub b: CandidateRecord,
    /// The thresholds the grade was read against.
    pub thresholds: Thresholds,
    /// Whether a link merges two persons into one.
    pub merges: bool,
    /// When the candidate was queued, RFC 3339 UTC.
    pub created_at: String,
    /// Who decided, when the candidate was decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    /// When it was decided, RFC 3339 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
    /// The reason given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The date a deferred candidate comes back, RFC 3339 UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deferred_until: Option<String>,
    /// The next candidate in the queue's order, after this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// The candidate before this one in the queue's order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
}

/// What a steward decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// The records are one patient.
    Link,
    /// The records are two patients.
    NotAMatch,
    /// Decide later, on the date `until` names.
    Defer,
}

/// The body of `POST {base}/operator/candidates/{id}/decide`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    /// What the steward decides.
    pub verdict: Verdict,
    /// For a deferral, the day the candidate comes back, `YYYY-MM-DD`,
    /// after today; absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    /// The version of the first record the steward read.
    pub a_version: i32,
    /// The version of the second record the steward read.
    pub b_version: i32,
    /// Why, required and at most 500 characters.
    pub reason: String,
}

/// What a decision answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decided {
    /// The candidate's id.
    pub candidate: String,
    /// Where it stands now.
    pub status: Status,
    /// The steward action that records the decision.
    pub action: String,
    /// The merge a link made, when it joined two persons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge: Option<String>,
    /// The person both records are in after a link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
    /// The next candidate in the queue's order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{Decision, Verdict};

    #[test]
    fn a_decision_reads_its_verdict_and_date_and_refuses_an_unknown_field() {
        let defer: Decision = serde_json::from_str(
            r#"{"verdict":"defer","until":"2026-11-01","a_version":2,"b_version":1,"reason":"ask the ward"}"#,
        )
        .expect("a deferral reads");
        assert_eq!(defer.verdict, Verdict::Defer);
        assert_eq!(defer.until.as_deref(), Some("2026-11-01"));
        let link: Decision = serde_json::from_str(
            r#"{"verdict":"link","a_version":1,"b_version":1,"reason":"same patient"}"#,
        )
        .expect("a link reads");
        assert_eq!(link.verdict, Verdict::Link);
        assert_eq!(link.until, None);
        assert!(
            serde_json::from_str::<Decision>(
                r#"{"verdict":"link","a_version":1,"b_version":1,"reason":"x","extra":1}"#
            )
            .is_err()
        );
    }
}
