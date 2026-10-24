// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The dashboard document, `GET {base}/operator/overview`.
//!
//! It holds the review queue, the records fed and linked, the subscriptions
//! in error, the queue depth over 30 days, the feeds per hour by channel over
//! 24 hours and the oldest candidates. It names no patient.

use serde::{Deserialize, Serialize};

use crate::patient::Grade;

/// The days the queue depth series covers, today included.
pub const QUEUE_DAYS: usize = 30;

/// The hours the feed series covers, the current one included.
pub const FEED_HOURS: usize = 24;

/// The most candidates [`Overview::oldest`] lists.
pub const OLDEST: usize = 5;

/// What `GET {base}/operator/overview` answers with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overview {
    /// The review queue now.
    pub queue: Queue,
    /// The source records created in the last 24 hours.
    pub records_created_24h: u64,
    /// Of those, the records whose person holds another record too: linked
    /// to another domain's record without a steward.
    pub records_linked_24h: u64,
    /// The record versions received since midnight UTC, on every channel.
    pub versions_fed_today: u64,
    /// The PMIR subscriptions in the `error` state.
    pub subscriptions_in_error: u64,
    /// The open candidates at the end of each of the last
    /// [`QUEUE_DAYS`] days (UTC), the oldest day first.
    pub queue_depth: Vec<DayCount>,
    /// The versions received in each of the last [`FEED_HOURS`] hours, the
    /// oldest hour first.
    pub feeds_per_hour: Vec<HourFeeds>,
    /// The open candidates waiting longest, at most [`OLDEST`].
    pub oldest: Vec<QueueEntry>,
}

/// The review queue's counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Queue {
    /// The candidates open for a decision.
    pub open: u64,
    /// Of those, the candidates waiting more than seven days.
    pub waiting_over_week: u64,
    /// The candidates a steward deferred.
    pub deferred: u64,
}

/// One day of the queue depth series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DayCount {
    /// The day, `YYYY-MM-DD` in UTC.
    pub day: String,
    /// The candidates open at the end of the day.
    pub open: u64,
}

/// One hour of the feed series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HourFeeds {
    /// The start of the hour, RFC 3339 UTC.
    pub hour: String,
    /// The versions received on the FHIR face.
    pub fhir: u64,
    /// The versions received on the HL7 v2 face.
    pub v2: u64,
    /// The versions received through PMIR.
    pub pmir: u64,
}

/// One candidate of the queue, as the dashboard lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueEntry {
    /// The candidate's id.
    pub candidate: String,
    /// When the candidate was queued, RFC 3339 UTC.
    pub created_at: String,
    /// The score, in `[0, 1]`.
    pub score: f64,
    /// The grade of the score.
    pub grade: Grade,
}
