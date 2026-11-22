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
use crate::store::evidence;
use crate::store::record::PersonId;

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
            Some(evidence::pack(&scored.evidence).map_err(|_long| FeedError::Evidence)?),
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
    let action = write::recorded(
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
             VALUES ($1, $2, $3, $4, $5, tstzrange(clock_timestamp(), NULL), $6)
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

/// Records each pair of `me` and another record in `pairs` as an open
/// candidate, or refreshes the open candidate of the pair, with the system
/// action of each, in one statement, and returns the candidates written in
/// the order of `pairs`; a pair a steward deferred is left as it is, a
/// pair a steward held to be two patients is not queued again while both
/// records state what they stated then, and an open candidate a version of
/// unchanged demographics scores to the same score, grade and evidence is
/// not written or recorded again.
///
/// A version that changes only what the scorer does not compare (an
/// identifier, a `meta` element) leaves a steward's not-a-match standing;
/// a change of the demographics queues the pair again
/// (`docs/architecture.md` §5.3).
pub(super) async fn candidates(
    connection: &mut PgConnection,
    me: &Me<'_>,
    pairs: &[(&Other, Scored)],
    context: &Context,
) -> Result<Vec<CandidateId>, FeedError> {
    if pairs.is_empty() {
        return Ok(Vec::new());
    }
    let mut record_a = Vec::with_capacity(pairs.len());
    let mut record_b = Vec::with_capacity(pairs.len());
    let mut a_version = Vec::with_capacity(pairs.len());
    let mut b_version = Vec::with_capacity(pairs.len());
    let mut score = Vec::with_capacity(pairs.len());
    let mut grade = Vec::with_capacity(pairs.len());
    let mut packed = Vec::with_capacity(pairs.len());
    for (other, scored) in pairs {
        let ((a, av), (b, bv)) = if me.record < other.record {
            ((me.record, me.version), (other.record, other.version))
        } else {
            ((other.record, other.version), (me.record, me.version))
        };
        record_a.push(a.uuid());
        record_b.push(b.uuid());
        a_version.push(av);
        b_version.push(bv);
        score.push(scored.score);
        grade.push(scored.grade.code());
        packed.push(evidence::pack(&scored.evidence).map_err(|_long| FeedError::Evidence)?);
    }
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "WITH pair AS (
             SELECT * FROM unnest($1::uuid[], $2::uuid[], $3::int[], $4::int[],
                                  $5::float8[], $6::text[], $7::bytea[])
                  WITH ORDINALITY AS p (record_a, record_b, a_version, b_version,
                                        score, grade, evidence, n)
         ), written AS (
             INSERT INTO pix.candidate (record_a, record_b, a_version, b_version, score, grade, evidence)
             SELECT p.record_a, p.record_b, p.a_version, p.b_version, p.score, p.grade, p.evidence
             FROM pair p
             WHERE NOT EXISTS (
                 SELECT 1 FROM pix.candidate c
                 JOIN pix.source_record_version da ON da.record_id = c.record_a AND da.version = c.a_version
                 JOIN pix.source_record_version db ON db.record_id = c.record_b AND db.version = c.b_version
                 JOIN pix.source_record_version na ON na.record_id = c.record_a AND na.version = p.a_version
                 JOIN pix.source_record_version nb ON nb.record_id = c.record_b AND nb.version = p.b_version
                 WHERE c.record_a = p.record_a AND c.record_b = p.record_b AND c.status = 'not-a-match'
                   AND da.demographics = na.demographics AND db.demographics = nb.demographics)
             ORDER BY p.n
             ON CONFLICT (record_a, record_b) WHERE status IN ('open', 'deferred')
             DO UPDATE SET a_version = EXCLUDED.a_version, b_version = EXCLUDED.b_version,
                           score = EXCLUDED.score, grade = EXCLUDED.grade, evidence = EXCLUDED.evidence
             WHERE pix.candidate.status = 'open'
               AND NOT ($8 AND pix.candidate.score = EXCLUDED.score
                        AND pix.candidate.grade = EXCLUDED.grade
                        AND pix.candidate.evidence = EXCLUDED.evidence)
             RETURNING id, record_a, record_b
         ), ordered AS (
             SELECT w.id, p.n FROM written w
             JOIN pair p ON p.record_a = w.record_a AND p.record_b = w.record_b
         ), recorded AS (
             INSERT INTO pix.steward_action (kind, actor, record_id, candidate_id, request_id)
             SELECT 'candidate', $9, $10, o.id, $11 FROM ordered o ORDER BY o.n
         )
         SELECT id FROM ordered ORDER BY n",
    )
    .bind(record_a)
    .bind(record_b)
    .bind(a_version)
    .bind(b_version)
    .bind(score)
    .bind(grade)
    .bind(packed)
    .bind(me.unchanged)
    .bind(&context.actor)
    .bind(me.record.uuid())
    .bind(context.request_id.as_deref())
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    Ok(ids.into_iter().map(CandidateId).collect())
}
