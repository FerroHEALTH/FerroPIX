// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A steward's decision on a candidate, in one transaction
//! (`docs/architecture.md` §4.3 and §5.3).
//!
//! The candidate is locked, and the decision is refused as stale when it is
//! no longer waiting or when either record moved past the version the
//! steward read. A link of two records in two persons merges the persons,
//! the older surviving; not a match and a deferral settle the candidate.
//! Each decision is one steward action with its reason, and nothing is
//! deleted. No specification governs the decisions: our own design.

use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

use crate::feed::link::{self, MergeKind, Recording};
use crate::feed::write::{self, Subjects};
use crate::feed::{Context, FeedError};
use crate::store::record::{PersonId, RecordId};
use crate::store::{Store, StoreError};

/// What a steward decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The records are one patient.
    Link,
    /// The records are two patients.
    NotAMatch,
    /// Decide on the day `until`, `YYYY-MM-DD`.
    Defer {
        /// The day the candidate comes back.
        until: jiff::civil::Date,
    },
}

/// One decision, as the steward made it.
#[derive(Debug, Clone)]
pub struct Decision {
    /// The candidate.
    pub candidate: Uuid,
    /// What the steward decides.
    pub verdict: Verdict,
    /// The version of the first record the steward read.
    pub a_version: i32,
    /// The version of the second record the steward read.
    pub b_version: i32,
    /// Why.
    pub reason: String,
}

/// What a decision did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decided {
    /// The steward action that records it.
    pub action: Uuid,
    /// The merge a link made.
    pub merge: Option<Uuid>,
    /// The person both records are in after a link.
    pub person: Option<PersonId>,
    /// The two records' persons before the decision, the first record's
    /// first.
    pub persons: Vec<PersonId>,
}

/// Why a decision was not made.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DecideError {
    /// No candidate has the id.
    #[error("no candidate has the id")]
    NotFound,
    /// The candidate was decided, or a record moved, since the steward read
    /// it.
    #[error("the candidate changed since it was read")]
    Stale,
    /// The store refused the decision.
    #[error("the store refused the decision")]
    Feed(#[from] FeedError),
}

impl From<StoreError> for DecideError {
    fn from(source: StoreError) -> Self {
        Self::Feed(FeedError::Store(source))
    }
}

impl From<sqlx::Error> for DecideError {
    fn from(source: sqlx::Error) -> Self {
        Self::Feed(FeedError::Store(source.into()))
    }
}

/// The candidate's row as the decision reads it.
type CandidateRow = (Uuid, Uuid, i32, i32, String, i32, i32);

/// Makes `decision` as `context`'s actor in one transaction on `store`,
/// and returns the transaction open, so the caller writes the audit record
/// before it commits.
///
/// # Errors
///
/// Returns [`DecideError::NotFound`] for an unknown candidate,
/// [`DecideError::Stale`] when it was decided or either record moved since
/// the versions the decision carries, and [`DecideError::Feed`] when the
/// store refuses a statement.
pub async fn decide(
    store: &Store,
    decision: &Decision,
    context: &Context,
) -> Result<(Transaction<'static, Postgres>, Decided), DecideError> {
    let mut transaction = store.pool().begin().await?;
    let decided = settle(&mut transaction, decision, context).await?;
    Ok((transaction, decided))
}

/// Settles `decision` on `connection`, inside its transaction.
async fn settle(
    connection: &mut PgConnection,
    decision: &Decision,
    context: &Context,
) -> Result<Decided, DecideError> {
    let row: Option<CandidateRow> = sqlx::query_as(
        "SELECT c.record_a, c.record_b, c.a_version, c.b_version, c.status,
                ra.head_version, rb.head_version
         FROM pix.candidate c
         JOIN pix.source_record ra ON ra.id = c.record_a
         JOIN pix.source_record rb ON rb.id = c.record_b
         WHERE c.id = $1
         FOR UPDATE OF c",
    )
    .bind(decision.candidate)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((a, b, a_version, b_version, status, a_head, b_head)) = row else {
        return Err(DecideError::NotFound);
    };
    let waiting = status == "open" || status == "deferred";
    let read = decision.a_version == a_version && decision.b_version == b_version;
    let current = a_head == a_version && b_head == b_version;
    if !(waiting && read && current) {
        return Err(DecideError::Stale);
    }
    let (a, b) = (RecordId::from_uuid(a), RecordId::from_uuid(b));
    let persons: Vec<PersonId> = [
        link::open_person(connection, a).await?,
        link::open_person(connection, b).await?,
    ]
    .into_iter()
    .flatten()
    .collect();
    let reason = Some(decision.reason.as_str());
    match decision.verdict {
        Verdict::Link => {
            let (merge, person, action) =
                link_pair(connection, decision.candidate, a, &persons, context, reason).await?;
            Ok(Decided {
                action,
                merge,
                person: Some(person),
                persons,
            })
        }
        Verdict::NotAMatch => {
            sqlx::query(
                "UPDATE pix.candidate
                 SET status = 'not-a-match', decided_by = $2, decided_at = now(),
                     reason = $3, deferred_until = NULL
                 WHERE id = $1",
            )
            .bind(decision.candidate)
            .bind(&context.actor)
            .bind(reason)
            .execute(&mut *connection)
            .await?;
            let action = recorded(connection, "not-a-match", decision, a, context).await?;
            Ok(Decided {
                action,
                merge: None,
                person: None,
                persons,
            })
        }
        Verdict::Defer { until } => {
            sqlx::query(
                "UPDATE pix.candidate
                 SET status = 'deferred', decided_by = $2, decided_at = now(),
                     reason = $3, deferred_until = ($4::date)::timestamptz
                 WHERE id = $1",
            )
            .bind(decision.candidate)
            .bind(&context.actor)
            .bind(reason)
            .bind(until.to_string())
            .execute(&mut *connection)
            .await?;
            let action = recorded(connection, "defer", decision, a, context).await?;
            Ok(Decided {
                action,
                merge: None,
                person: None,
                persons,
            })
        }
    }
}

/// Links the pair of the candidate `candidate`: merges the two persons of
/// `persons` into the older, or settles the candidate when both records are
/// in one person, and returns the merge, the person and the action.
async fn link_pair(
    connection: &mut PgConnection,
    candidate: Uuid,
    a: RecordId,
    persons: &[PersonId],
    context: &Context,
    reason: Option<&str>,
) -> Result<(Option<Uuid>, PersonId, Uuid), DecideError> {
    let mut sorted = persons.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    match sorted.as_slice() {
        [survivor, merged] => {
            link::lock_persons(connection, &sorted)
                .await
                .map_err(stale)?;
            let recording = Recording {
                context,
                kind: "link",
                candidate: Some(candidate),
                reason,
            };
            let made = link::merge_recorded(
                connection,
                *merged,
                *survivor,
                MergeKind::Steward,
                recording,
            )
            .await
            .map_err(stale)?;
            sqlx::query("UPDATE pix.candidate SET reason = $2 WHERE id = $1")
                .bind(candidate)
                .bind(reason)
                .execute(&mut *connection)
                .await?;
            Ok((Some(made.merge), *survivor, made.action))
        }
        [person] => {
            sqlx::query(
                "UPDATE pix.candidate
                 SET status = 'linked', decided_by = $2, decided_at = now(),
                     reason = $3, deferred_until = NULL
                 WHERE id = $1",
            )
            .bind(candidate)
            .bind(&context.actor)
            .bind(reason)
            .execute(&mut *connection)
            .await?;
            let action = write::reasoned(
                connection,
                "link",
                context,
                Subjects {
                    person: Some(*person),
                    record: Some(a),
                    candidate: Some(candidate),
                    merge: None,
                },
                reason,
            )
            .await?;
            Ok((None, *person, action))
        }
        _ => Err(DecideError::Stale),
    }
}

/// Records the steward action `kind` of `decision`.
async fn recorded(
    connection: &mut PgConnection,
    kind: &'static str,
    decision: &Decision,
    a: RecordId,
    context: &Context,
) -> Result<Uuid, DecideError> {
    let action = write::reasoned(
        connection,
        kind,
        context,
        Subjects {
            person: None,
            record: Some(a),
            candidate: Some(decision.candidate),
            merge: None,
        },
        Some(decision.reason.as_str()),
    )
    .await?;
    Ok(action)
}

/// Reads a person no longer active, or a merge a concurrent transaction
/// made first, as a stale decision.
fn stale(error: FeedError) -> DecideError {
    match error {
        FeedError::Contention => DecideError::Stale,
        other => DecideError::Feed(other),
    }
}
