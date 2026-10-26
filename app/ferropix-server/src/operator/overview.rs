// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! `GET {base}/operator/overview`: the dashboard, read in one snapshot.
//!
//! The counts and the two series are read from the store as it is: the
//! queue from `candidate`, the feeds from `source_record_version`, the links
//! from `link` and the subscriptions from `subscription`. Every day and hour
//! is UTC. The document names no patient, so reading it writes no audit
//! record. What the dashboard counts is our own design: no specification
//! governs it.

use axum::response::Response;
use ferropix_operator_api::overview::{
    DayCount, FEED_HOURS, HourFeeds, OLDEST, Overview, QUEUE_DAYS, Queue, QueueEntry,
};
use sqlx::PgConnection;

use crate::operator::{Operator, OperatorError, count, grade, snapshot, wire};

/// The counts of the dashboard's stat cards: open, waiting over a week,
/// deferred, created, linked, fed today, subscriptions in error.
type Counts = (i64, i64, i64, i64, i64, i64, i64);

/// Answers the dashboard.
pub async fn answer(operator: &Operator) -> Response {
    match read(operator).await {
        Ok(overview) => wire::answer(&overview),
        Err(error) => wire::failed(&error),
    }
}

/// Reads the dashboard.
///
/// # Errors
///
/// Returns [`OperatorError`] when the store cannot be read.
pub async fn read(operator: &Operator) -> Result<Overview, OperatorError> {
    let mut snapshot = snapshot(&operator.store).await?;
    let (open, waiting, deferred, created, linked, fed, in_error): Counts = sqlx::query_as(
        "SELECT
             (SELECT count(*) FROM pix.candidate WHERE status = 'open'),
             (SELECT count(*) FROM pix.candidate
              WHERE status = 'open' AND created_at < now() - interval '7 days'),
             (SELECT count(*) FROM pix.candidate WHERE status = 'deferred'),
             (SELECT count(*) FROM pix.source_record
              WHERE created_at >= now() - interval '24 hours'),
             (SELECT count(DISTINCT l.record_id) FROM pix.link l
              JOIN pix.source_record r ON r.id = l.record_id
              WHERE l.kind = 'probabilistic'
                AND lower(l.valid) >= now() - interval '24 hours'
                AND r.created_at >= now() - interval '24 hours'),
             (SELECT count(*) FROM pix.source_record_version
              WHERE received_at >= date_trunc('day', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'),
             (SELECT count(*) FROM pix.subscription WHERE status = 'error' AND deleted_at IS NULL)",
    )
    .fetch_one(&mut *snapshot)
    .await?;
    let queue_depth = queue_depth(&mut snapshot).await?;
    let feeds_per_hour = feeds_per_hour(&mut snapshot).await?;
    let oldest = oldest(&mut snapshot).await?;
    snapshot.commit().await?;
    Ok(Overview {
        queue: Queue {
            open: count(open),
            waiting_over_week: count(waiting),
            deferred: count(deferred),
        },
        records_created_24h: count(created),
        records_linked_24h: count(linked),
        versions_fed_today: count(fed),
        subscriptions_in_error: count(in_error),
        queue_depth,
        feeds_per_hour,
        oldest,
    })
}

/// Reads the open candidates at the end of each of the last
/// [`QUEUE_DAYS`] days.
async fn queue_depth(connection: &mut PgConnection) -> Result<Vec<DayCount>, OperatorError> {
    let days = i32::try_from(QUEUE_DAYS)
        .map_err(|_overflow| OperatorError::Unadmitted("day count"))?
        .saturating_sub(1);
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT to_char(d, 'YYYY-MM-DD'),
                (SELECT count(*) FROM pix.candidate c
                 WHERE c.created_at < (d + interval '1 day') AT TIME ZONE 'UTC'
                   AND (c.decided_at IS NULL
                        OR c.decided_at >= (d + interval '1 day') AT TIME ZONE 'UTC'))
         FROM generate_series(
                  ((now() AT TIME ZONE 'UTC')::date - $1)::timestamp,
                  (now() AT TIME ZONE 'UTC')::date::timestamp,
                  interval '1 day'
              ) AS d
         ORDER BY d",
    )
    .bind(days)
    .fetch_all(&mut *connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(day, open)| DayCount {
            day,
            open: count(open),
        })
        .collect())
}

/// Reads the versions received in each of the last [`FEED_HOURS`] hours,
/// by channel.
async fn feeds_per_hour(connection: &mut PgConnection) -> Result<Vec<HourFeeds>, OperatorError> {
    let hours = i32::try_from(FEED_HOURS)
        .map_err(|_overflow| OperatorError::Unadmitted("hour count"))?
        .saturating_sub(1);
    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT to_char(h, 'YYYY-MM-DD\"T\"HH24:00:00\"Z\"'),
                count(v.id) FILTER (WHERE v.channel = 'fhir'),
                count(v.id) FILTER (WHERE v.channel = 'v2'),
                count(v.id) FILTER (WHERE v.channel = 'pmir')
         FROM generate_series(
                  date_trunc('hour', now() AT TIME ZONE 'UTC') - make_interval(hours => $1),
                  date_trunc('hour', now() AT TIME ZONE 'UTC'),
                  interval '1 hour'
              ) AS h
         LEFT JOIN pix.source_record_version v
                ON v.received_at >= h AT TIME ZONE 'UTC'
               AND v.received_at < (h + interval '1 hour') AT TIME ZONE 'UTC'
         GROUP BY h
         ORDER BY h",
    )
    .bind(hours)
    .fetch_all(&mut *connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(hour, fhir, v2, pmir)| HourFeeds {
            hour,
            fhir: count(fhir),
            v2: count(v2),
            pmir: count(pmir),
        })
        .collect())
}

/// Reads the open candidates waiting longest, at most [`OLDEST`].
async fn oldest(connection: &mut PgConnection) -> Result<Vec<QueueEntry>, OperatorError> {
    let limit = i64::try_from(OLDEST).map_err(|_overflow| OperatorError::Unadmitted("limit"))?;
    let rows: Vec<(uuid::Uuid, String, f64, String)> = sqlx::query_as(
        "SELECT id,
                to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
                score, grade
         FROM pix.candidate WHERE status = 'open'
         ORDER BY created_at, id
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&mut *connection)
    .await?;
    rows.into_iter()
        .map(|(id, created_at, score, stored)| {
            Ok(QueueEntry {
                candidate: id.to_string(),
                created_at,
                score,
                grade: grade(&stored)?,
            })
        })
        .collect()
}
