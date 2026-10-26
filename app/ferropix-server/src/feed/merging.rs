// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A steward's merge of two persons and its revert, each in one transaction
//! (`docs/architecture.md` §4.3).
//!
//! Both persons are locked in key order and the change is refused as stale
//! when a person is no longer in the state the steward read or its version
//! moved. A merge moves every open link of the merged person onto the
//! survivor with a `merge_effect` row; a revert replays those rows
//! backwards and is allowed only for the latest unreverted merge whose
//! survivor is the person, so reverts unwind in order. PMIR 1.6.0
//! §2:3.93.4.1.3 admits a local unmerge policy: a steward may unmerge here
//! and a Source may not through the feed. The rest is our own design.

use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

use crate::feed::link::{self, MergeKind, Recording};
use crate::feed::steward::DecideError;
use crate::feed::write::{self, Subjects};
use crate::feed::{Context, FeedError, announce};
use crate::store::Store;
use crate::store::record::PersonId;

/// The format a person's version is written in: the time of its latest
/// change, to the microsecond, UTC.
const VERSION: &str = "YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"";

/// A merge as the steward asked for it.
#[derive(Debug, Clone)]
pub struct MergeRequest {
    /// The person that survives.
    pub survivor: PersonId,
    /// The survivor's version as the steward read it.
    pub survivor_version: String,
    /// The person merged into it.
    pub merged: PersonId,
    /// The merged person's version as the steward read it.
    pub merged_version: String,
    /// Why.
    pub reason: String,
}

/// What a merge or a revert changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The merge made or reverted.
    pub merge: Uuid,
    /// The steward action that records it.
    pub action: Uuid,
    /// The surviving person.
    pub survivor: PersonId,
    /// The person merged, or reactivated.
    pub merged: PersonId,
    /// How many records moved.
    pub moved: u64,
}

/// Why a revert was not made, beyond a decision's reasons.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RevertError {
    /// A later merge into the same survivor has to be reverted first.
    #[error("a later merge of the survivor has to be reverted first")]
    NotLatest {
        /// That later merge.
        latest: Uuid,
    },
    /// The merge is not known, was reverted, or a person moved.
    #[error(transparent)]
    Decide(#[from] DecideError),
}

impl From<sqlx::Error> for RevertError {
    fn from(source: sqlx::Error) -> Self {
        Self::Decide(source.into())
    }
}

/// Returns the version of `person`, or `None` for an unknown person.
///
/// The version is the time of the person's latest change: a link opened or
/// closed, or a version fed to one of its records. A merge and a revert
/// compare it with what the steward read.
///
/// # Errors
///
/// Returns [`FeedError::Store`] when the statement fails.
pub async fn version(
    connection: &mut PgConnection,
    person: PersonId,
) -> Result<Option<String>, FeedError> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT to_char(greatest(
                    p.created_at,
                    (SELECT max(greatest(lower(l.valid), upper(l.valid)))
                     FROM pix.link l WHERE l.person_id = p.id),
                    (SELECT max(v.received_at)
                     FROM pix.link l
                     JOIN pix.source_record_version v ON v.record_id = l.record_id
                     WHERE l.person_id = p.id AND upper_inf(l.valid))
                ) AT TIME ZONE 'UTC', '{VERSION}')
         FROM pix.person p WHERE p.id = $1"
    )))
    .bind(person.uuid())
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| FeedError::Store(error.into()))
}

/// Merges as `request` asks, as `context`'s actor, and returns the
/// transaction open so the caller writes the audit record before it
/// commits.
///
/// # Errors
///
/// Returns [`DecideError::NotFound`] when either person is unknown,
/// [`DecideError::Stale`] when either is no longer active or its version
/// moved, and [`DecideError::Feed`] when the store refuses a statement.
pub async fn merge(
    store: &Store,
    request: &MergeRequest,
    context: &Context,
) -> Result<(Transaction<'static, Postgres>, Change), DecideError> {
    let mut transaction = store.pool().begin().await?;
    let persons = [request.survivor, request.merged];
    write::advisory(
        &mut transaction,
        persons
            .iter()
            .map(|person| write::person_key(*person))
            .collect(),
    )
    .await?;
    for (person, read) in [
        (request.survivor, &request.survivor_version),
        (request.merged, &request.merged_version),
    ] {
        let status = status(&mut transaction, person).await?;
        let Some((status, _merged_into)) = status else {
            return Err(DecideError::NotFound);
        };
        let current = version(&mut transaction, person).await?;
        if status != "active" || current.as_ref() != Some(read) {
            return Err(DecideError::Stale);
        }
    }
    let recording = Recording {
        context,
        kind: "merge",
        candidate: None,
        reason: Some(request.reason.as_str()),
    };
    let made = link::merge_recorded(
        &mut transaction,
        request.merged,
        request.survivor,
        MergeKind::Steward,
        recording,
    )
    .await
    .map_err(|error| match error {
        FeedError::Contention => DecideError::Stale,
        other => DecideError::Feed(other),
    })?;
    let moved = effects(&mut transaction, made.merge).await?;
    let changes = [
        announce::Change::Merged(request.merged),
        announce::Change::Updated(request.survivor),
    ];
    announce::announce(&mut transaction, store, &changes).await?;
    Ok((
        transaction,
        Change {
            merge: made.merge,
            action: made.action,
            survivor: request.survivor,
            merged: request.merged,
            moved,
        },
    ))
}

/// Reverts the merge `merge` as `context`'s actor, with the survivor's
/// version `read` and the steward's `reason`, and returns the transaction
/// open so the caller writes the audit record before it commits.
///
/// # Errors
///
/// Returns [`RevertError::NotLatest`] when a later merge of the survivor is
/// unreverted, [`DecideError::NotFound`] for an unknown merge,
/// [`DecideError::Stale`] when the merge was reverted, a person moved or a
/// moved record's link changed since, and [`DecideError::Feed`] when the
/// store refuses a statement.
pub async fn revert(
    store: &Store,
    merge: Uuid,
    read: &str,
    reason: &str,
    context: &Context,
) -> Result<(Transaction<'static, Postgres>, Change), RevertError> {
    let mut transaction = store.pool().begin().await?;
    let row: Option<(Uuid, Uuid, bool)> = sqlx::query_as(
        "SELECT survivor_id, merged_id, reverted_at IS NOT NULL FROM pix.merge WHERE id = $1 FOR UPDATE",
    )
    .bind(merge)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some((survivor, merged, reverted)) = row else {
        return Err(DecideError::NotFound.into());
    };
    if reverted {
        return Err(DecideError::Stale.into());
    }
    let (survivor, merged) = (PersonId::from_uuid(survivor), PersonId::from_uuid(merged));
    write::advisory(
        &mut transaction,
        vec![write::person_key(survivor), write::person_key(merged)],
    )
    .await
    .map_err(|error| DecideError::Feed(error.into()))?;
    if let Some(latest) = latest(&mut transaction, survivor).await?
        && latest != merge
    {
        return Err(RevertError::NotLatest { latest });
    }
    let survivor_state = status(&mut transaction, survivor).await?;
    let merged_state = status(&mut transaction, merged).await?;
    let in_place = matches!(survivor_state, Some((ref status, None)) if status == "active")
        && matches!(merged_state, Some((ref status, Some(into))) if status == "merged" && into == survivor.uuid());
    let current = version(&mut transaction, survivor)
        .await
        .map_err(DecideError::Feed)?;
    if !in_place || current.as_deref() != Some(read) {
        return Err(DecideError::Stale.into());
    }
    let action = write::reasoned(
        &mut transaction,
        "unmerge",
        context,
        Subjects {
            person: Some(merged),
            record: None,
            candidate: None,
            merge: Some(merge),
        },
        Some(reason),
    )
    .await
    .map_err(|error| DecideError::Feed(error.into()))?;
    let moved = replay(&mut transaction, merge, action, context).await?;
    sqlx::query("UPDATE pix.person SET status = 'active', merged_into = NULL WHERE id = $1")
        .bind(merged.uuid())
        .execute(&mut *transaction)
        .await?;
    let changes = [
        announce::Change::Updated(survivor),
        announce::Change::Updated(merged),
    ];
    announce::announce(&mut transaction, store, &changes)
        .await
        .map_err(DecideError::Feed)?;
    sqlx::query(
        "UPDATE pix.merge SET reverted_at = now(), reverted_by = $2, revert_reason = $3 WHERE id = $1",
    )
    .bind(merge)
    .bind(&context.actor)
    .bind(reason)
    .execute(&mut *transaction)
    .await?;
    Ok((
        transaction,
        Change {
            merge,
            action,
            survivor,
            merged,
            moved,
        },
    ))
}

/// Replays the effects of `merge` backwards: each link the merge opened is
/// closed by `action` and the record's link to the person it came from is
/// opened again; returns how many records moved back.
async fn replay(
    connection: &mut PgConnection,
    merge: Uuid,
    action: Uuid,
    context: &Context,
) -> Result<u64, RevertError> {
    let rows: Vec<(Uuid, Uuid, Uuid)> = sqlx::query_as(
        "SELECT record_id, from_person, link_opened FROM pix.merge_effect
         WHERE merge_id = $1 ORDER BY record_id DESC",
    )
    .bind(merge)
    .fetch_all(&mut *connection)
    .await?;
    for (record, from, opened) in &rows {
        let closed = sqlx::query(
            "UPDATE pix.link SET valid = tstzrange(lower(valid), clock_timestamp()), ended_by_action = $2
             WHERE id = $1 AND upper_inf(valid)",
        )
        .bind(opened)
        .bind(action)
        .execute(&mut *connection)
        .await?
        .rows_affected();
        if closed != 1 {
            return Err(DecideError::Stale.into());
        }
        sqlx::query(
            "INSERT INTO pix.link (record_id, person_id, kind, valid, made_by)
             VALUES ($1, $2, 'steward', tstzrange(clock_timestamp(), NULL), $3)",
        )
        .bind(record)
        .bind(from)
        .bind(&context.actor)
        .execute(&mut *connection)
        .await?;
    }
    Ok(u64::try_from(rows.len()).unwrap_or(u64::MAX))
}

/// Returns the latest unreverted merge whose survivor is `survivor`.
///
/// # Errors
///
/// Returns the driver's error when the statement fails.
pub async fn latest(
    connection: &mut PgConnection,
    survivor: PersonId,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM pix.merge WHERE survivor_id = $1 AND reverted_at IS NULL
         ORDER BY made_at DESC, id DESC LIMIT 1",
    )
    .bind(survivor.uuid())
    .fetch_optional(&mut *connection)
    .await
}

/// Returns the status of `person` and the survivor it was merged into.
async fn status(
    connection: &mut PgConnection,
    person: PersonId,
) -> Result<Option<(String, Option<Uuid>)>, sqlx::Error> {
    sqlx::query_as("SELECT status, merged_into FROM pix.person WHERE id = $1")
        .bind(person.uuid())
        .fetch_optional(&mut *connection)
        .await
}

/// Returns how many records `merge` moved.
async fn effects(connection: &mut PgConnection, merge: Uuid) -> Result<u64, sqlx::Error> {
    let moved: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pix.merge_effect WHERE merge_id = $1")
            .bind(merge)
            .fetch_one(&mut *connection)
            .await?;
    Ok(moved.unsigned_abs())
}
