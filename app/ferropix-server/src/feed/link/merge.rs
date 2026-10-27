// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The merges a feed or a steward makes: the persons merged into the
//! survivor, the records moved, the candidates closed, and the steward
//! action that records it.

use sqlx::PgConnection;
use uuid::Uuid;

use crate::feed::write::{self, Subjects};
use crate::feed::{CandidateId, Context, FeedError};
use crate::matching::scorer::Scored;
use crate::store::record::{PersonId, RecordId};

use super::{LinkKind, Me, MergeKind, Other, store};

/// How a merge is recorded: the system action of a feed, or a steward's
/// decision with its kind, its candidate and its reason.
#[derive(Debug, Clone, Copy)]
pub(in crate::feed) struct Recording<'a> {
    /// The actor, the channel and the request.
    pub(in crate::feed) context: &'a Context,
    /// The steward action's kind.
    pub(in crate::feed) kind: &'static str,
    /// The candidate the decision settles.
    pub(in crate::feed) candidate: Option<Uuid>,
    /// The reason the steward gave.
    pub(in crate::feed) reason: Option<&'a str>,
}

/// What a merge wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::feed) struct Merged {
    /// The merge row.
    pub(in crate::feed) merge: Uuid,
    /// The steward action that records it.
    pub(in crate::feed) action: Uuid,
}

/// Merges `merged` into `survivor` as a system action of the feed.
pub(in crate::feed) async fn merge(
    connection: &mut PgConnection,
    merged: PersonId,
    survivor: PersonId,
    kind: MergeKind,
    context: &Context,
) -> Result<(), FeedError> {
    let recording = Recording {
        context,
        kind: "merge",
        candidate: None,
        reason: None,
    };
    merge_recorded(connection, merged, survivor, kind, recording)
        .await
        .map(|_merged| ())
}

/// What the links a merge moves are reopened as.
#[derive(Debug, Clone, Copy)]
pub(super) enum Moved<'a> {
    /// Links of kind `merge`.
    Merge,
    /// Links of kind `probabilistic`, with the score and the evidence that
    /// joined the persons.
    Scored(&'a Scored),
}

/// Merges `merged` into `survivor`, both locked and active: the merged
/// person points at the survivor, each of its open links is closed and
/// reopened on the survivor with a `merge_effect` row, the candidates the
/// merge decides are closed as linked, and the action is recorded as
/// `recording` says.
pub(in crate::feed) async fn merge_recorded(
    connection: &mut PgConnection,
    merged: PersonId,
    survivor: PersonId,
    kind: MergeKind,
    recording: Recording<'_>,
) -> Result<Merged, FeedError> {
    merge_moving(
        connection,
        (merged, survivor),
        kind,
        recording,
        Moved::Merge,
    )
    .await
}

/// Merges as [`merge_recorded`] does, the moved links reopened as `moved`
/// says.
pub(super) async fn merge_moving(
    connection: &mut PgConnection,
    (merged, survivor): (PersonId, PersonId),
    kind: MergeKind,
    recording: Recording<'_>,
    moved: Moved<'_>,
) -> Result<Merged, FeedError> {
    let context = recording.context;
    let (link_kind, score, evidence) = match moved {
        Moved::Merge => (LinkKind::Merge, None, None),
        Moved::Scored(scored) => (
            LinkKind::Probabilistic,
            Some(scored.score),
            Some(serde_json::to_string(&scored.evidence).map_err(|_json| FeedError::Evidence)?),
        ),
    };
    let merge: Uuid = sqlx::query_scalar(
        "INSERT INTO pix.merge (survivor_id, merged_id, kind, made_by, reason)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(survivor.uuid())
    .bind(merged.uuid())
    .bind(kind.as_str())
    .bind(&context.actor)
    .bind(recording.reason)
    .fetch_one(&mut *connection)
    .await
    .map_err(store)?;
    let action = write::reasoned(
        connection,
        recording.kind,
        context,
        Subjects {
            person: Some(survivor),
            candidate: recording.candidate,
            merge: Some(merge),
            ..Subjects::default()
        },
        recording.reason,
    )
    .await?;
    let moved = sqlx::query(
        "UPDATE pix.person SET status = 'merged', merged_into = $2
         WHERE id = $1 AND status = 'active'",
    )
    .bind(merged.uuid())
    .bind(survivor.uuid())
    .execute(&mut *connection)
    .await
    .map_err(store)?
    .rows_affected();
    if moved != 1 {
        return Err(FeedError::Contention);
    }
    let closed: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "UPDATE pix.link SET valid = tstzrange(lower(valid), clock_timestamp()), ended_by_action = $2
         WHERE person_id = $1 AND upper_inf(valid)
         RETURNING id, record_id",
    )
    .bind(merged.uuid())
    .bind(action)
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    for (closed, record) in closed {
        let opened: Uuid = sqlx::query_scalar(
            "INSERT INTO pix.link (record_id, person_id, kind, score, evidence, valid, made_by)
             VALUES ($1, $2, $3, $4, $5::jsonb, tstzrange(clock_timestamp(), NULL), $6)
             RETURNING id",
        )
        .bind(record)
        .bind(survivor.uuid())
        .bind(link_kind.as_str())
        .bind(score)
        .bind(evidence.as_deref())
        .bind(&context.actor)
        .fetch_one(&mut *connection)
        .await
        .map_err(store)?;
        sqlx::query(
            "INSERT INTO pix.merge_effect (merge_id, record_id, from_person, link_closed, link_opened)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(merge)
        .bind(record)
        .bind(merged.uuid())
        .bind(closed)
        .bind(opened)
        .execute(&mut *connection)
        .await
        .map_err(store)?;
    }
    close_candidates(connection, survivor, &context.actor).await?;
    Ok(Merged { merge, action })
}

/// Closes as linked every open or deferred candidate whose two records are
/// both in `survivor`, decided by `actor`.
pub(super) async fn close_candidates(
    connection: &mut PgConnection,
    survivor: PersonId,
    actor: &str,
) -> Result<(), FeedError> {
    sqlx::query(
        "UPDATE pix.candidate c
         SET status = 'linked', decided_by = $2, decided_at = now(), deferred_until = NULL
         WHERE c.status IN ('open', 'deferred')
           AND EXISTS (SELECT 1 FROM pix.link l
                       WHERE l.record_id = c.record_a AND upper_inf(l.valid) AND l.person_id = $1)
           AND EXISTS (SELECT 1 FROM pix.link l
                       WHERE l.record_id = c.record_b AND upper_inf(l.valid) AND l.person_id = $1)",
    )
    .bind(survivor.uuid())
    .bind(actor)
    .execute(&mut *connection)
    .await
    .map_err(store)?;
    Ok(())
}

/// Records the pair of `me` and `other` as an open candidate, or refreshes
/// the open candidate of the pair, and records the system action; a pair
/// a steward deferred is left as it is, and a pair a steward held to be
/// two patients is not queued again while both records state what they
/// stated then.
pub(super) async fn candidate(
    connection: &mut PgConnection,
    me: &Me<'_>,
    other: &Other,
    scored: &Scored,
    context: &Context,
) -> Result<Option<CandidateId>, FeedError> {
    let ((a, a_version), (b, b_version)) = if me.record < other.record {
        ((me.record, me.version), (other.record, other.version))
    } else {
        ((other.record, other.version), (me.record, me.version))
    };
    if decided_apart(connection, (a, a_version), (b, b_version)).await? {
        return Ok(None);
    }
    let evidence = serde_json::to_string(&scored.evidence).map_err(|_json| FeedError::Evidence)?;
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO pix.candidate (record_a, record_b, a_version, b_version, score, grade, evidence)
         VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)
         ON CONFLICT (record_a, record_b) WHERE status IN ('open', 'deferred')
         DO UPDATE SET a_version = EXCLUDED.a_version, b_version = EXCLUDED.b_version,
                       score = EXCLUDED.score, grade = EXCLUDED.grade, evidence = EXCLUDED.evidence
         WHERE pix.candidate.status = 'open'
         RETURNING id",
    )
    .bind(a.uuid())
    .bind(b.uuid())
    .bind(a_version)
    .bind(b_version)
    .bind(scored.score)
    .bind(scored.grade.code())
    .bind(evidence)
    .fetch_optional(&mut *connection)
    .await
    .map_err(store)?;
    let Some(id) = id else {
        return Ok(None);
    };
    write::action(
        connection,
        "candidate",
        context,
        Subjects {
            record: Some(me.record),
            candidate: Some(id),
            ..Subjects::default()
        },
    )
    .await?;
    Ok(Some(CandidateId(id)))
}

/// Whether a steward held the records `a` and `b` to be two patients on
/// versions whose demographics their versions `a.1` and `b.1` still state.
///
/// A version that changes only what the scorer does not compare (an
/// identifier, a `meta` element) leaves the decision standing; a change of
/// the demographics queues the pair again (`docs/architecture.md` §5.3).
pub(super) async fn decided_apart(
    connection: &mut PgConnection,
    a: (RecordId, i32),
    b: (RecordId, i32),
) -> Result<bool, FeedError> {
    sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM pix.candidate c
             JOIN pix.source_record_version da ON da.record_id = c.record_a AND da.version = c.a_version
             JOIN pix.source_record_version db ON db.record_id = c.record_b AND db.version = c.b_version
             JOIN pix.source_record_version na ON na.record_id = c.record_a AND na.version = $3
             JOIN pix.source_record_version nb ON nb.record_id = c.record_b AND nb.version = $4
             WHERE c.record_a = $1 AND c.record_b = $2 AND c.status = 'not-a-match'
               AND da.demographics = na.demographics AND db.demographics = nb.demographics)",
    )
    .bind(a.0.uuid())
    .bind(b.0.uuid())
    .bind(a.1)
    .bind(b.1)
    .fetch_one(&mut *connection)
    .await
    .map_err(store)
}
