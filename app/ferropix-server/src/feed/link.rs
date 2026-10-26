// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The cross-referencing a feed runs over one record version, inside its
//! transaction.
//!
//! The deterministic tier (`docs/architecture.md` §5.1): every other record
//! whose current version carries one of this version's `authoritative`
//! identifiers is the same patient, unless the two hold a hard conflict,
//! which becomes a candidate instead. The persons named are locked in key
//! order, the oldest survives, and every other is merged into it. The
//! probabilistic tier (§5.2): every record sharing a blocking key with this
//! version, in another person, is scored, and a pair graded `possible` or
//! better is an open candidate, linked alone only when `auto_link` is on and
//! the pair grades `certain`. PIXm leaves the cross-referencing to the
//! Manager (PIXm 3.1.0 §2:3.104.4.1.3): the rules are our own design.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgConnection;
use uuid::Uuid;

use crate::config::section::domain::DomainKind;
use crate::config::section::matching::BlockingKey;
use crate::feed::write::{self, Subjects};
use crate::feed::{CandidateId, Context, FeedError, Feeder, demographics};
use crate::matching::conflict::{self, Conflict};
use crate::matching::scorer::{Grade, Scored};
use crate::matching::subject::{IdentifierKey, Subject};
use crate::store::record::{PersonId, RecordId};

/// The kinds of link the feed opens, as the schema spells them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LinkKind {
    /// The record's first person, made when it arrived alone.
    Feed,
    /// An authoritative identifier agreed.
    Deterministic,
    /// Moved by a person merge.
    Merge,
    /// The scorer graded the pair certain with `auto_link` on.
    Probabilistic,
}

impl LinkKind {
    /// Returns the stored spelling.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Feed => "feed",
            Self::Deterministic => "deterministic",
            Self::Merge => "merge",
            Self::Probabilistic => "probabilistic",
        }
    }
}

/// Why two persons are merged, as the schema spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MergeKind {
    /// The Source resolved a duplicate (PIXm 3.1.0 §2:3.104.4.2.3).
    Source,
    /// A shared authoritative identifier.
    Deterministic,
    /// An automatic probabilistic link.
    Probabilistic,
    /// A steward's decision.
    Steward,
}

impl MergeKind {
    /// Returns the stored spelling.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Deterministic => "deterministic",
            Self::Probabilistic => "probabilistic",
            Self::Steward => "steward",
        }
    }
}

/// The version the cross-referencing runs over.
#[derive(Debug)]
pub(super) struct Me<'a> {
    /// The record.
    pub record: RecordId,
    /// Its version.
    pub version: i32,
    /// Whether it is a record of the master domain, a master identity.
    pub master: bool,
    /// The person a master record names by its value, which an ITI-93
    /// create links it to, made with this id when the store lacks it.
    pub identity: Option<PersonId>,
    /// Its demographics as the scorer compares them.
    pub subject: &'a Subject,
    /// The identifiers it carries in `authoritative` domains.
    pub authoritative: Vec<IdentifierKey>,
    /// The identifiers it carries in `local` domains.
    pub local: Vec<IdentifierKey>,
    /// Its `(birth year, phonetic key)` blocking key.
    pub phonetic: Option<String>,
    /// Its `(postal code, birth date)` blocking key.
    pub postal: Option<String>,
}

/// What the cross-referencing did.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Linked {
    /// The person the record is in afterwards.
    pub person: Option<PersonId>,
    /// The persons merged into it.
    pub merged: Vec<PersonId>,
    /// The candidates recorded or refreshed.
    pub candidates: Vec<CandidateId>,
    /// The pairs the probabilistic tier scored.
    pub scored: usize,
}

/// Another record the cross-referencing compares with.
#[derive(Debug)]
struct Other {
    /// The record.
    record: RecordId,
    /// Its current version.
    version: i32,
    /// The person its open link names.
    person: PersonId,
    /// Its current demographics.
    subject: Subject,
}

/// Runs both tiers over `me` for `feeder`, recording each system action as
/// `context`'s.
pub(super) async fn cross_reference(
    connection: &mut PgConnection,
    feeder: &Feeder,
    me: &Me<'_>,
    context: &Context,
) -> Result<Linked, FeedError> {
    let mut linked = Linked::default();
    let survivor = join(connection, feeder, me, context, &mut linked).await?;
    let survivor = probabilistic(connection, feeder, me, survivor, context, &mut linked).await?;
    linked.person = Some(survivor);
    Ok(linked)
}

/// The deterministic tier: links `me` into the person that survives the
/// others its authoritative identifiers name, merges those into it, records
/// a candidate for each record a hard conflict holds apart, and returns the
/// person `me` is in.
///
/// A master identity is never merged into another by matching: a master
/// record keeps its own person, a second master person the identifiers name
/// is held apart as a candidate with the [`Conflict::MasterIdentities`]
/// conflict, and only the Source's own merge joins them (PMIR 1.6.0
/// §2:3.93.4.1.3).
async fn join(
    connection: &mut PgConnection,
    feeder: &Feeder,
    me: &Me<'_>,
    context: &Context,
    linked: &mut Linked,
) -> Result<PersonId, FeedError> {
    let current = open_person(connection, me.record).await?;
    let matches = deterministic(connection, me).await?;
    let mut conflicted: BTreeSet<PersonId> = BTreeSet::new();
    let mut conflicting = Vec::new();
    for other in &matches {
        let found = conflict::conflicts(me.subject, &other.subject);
        if !found.is_empty() {
            conflicted.insert(other.person);
            conflicting.push((other, found));
        }
    }
    let joinable: BTreeSet<PersonId> = matches
        .iter()
        .map(|other| other.person)
        .filter(|person| !conflicted.contains(person))
        .collect();
    let identity = match (current, me.identity) {
        (None, Some(identity)) => Some(super::master::person_with_id(connection, identity).await?),
        _ => None,
    };
    let mut persons: Vec<PersonId> = joinable
        .iter()
        .copied()
        .chain(current)
        .chain(identity)
        .collect();
    persons.sort_unstable();
    persons.dedup();
    lock_persons(connection, &persons).await?;
    let ranked = by_survival(connection, &persons).await?;
    let current_master = current.is_some_and(|current| {
        ranked
            .iter()
            .any(|(person, master)| *person == current && *master)
    });
    let survivor = if me.master || current_master {
        match current.or(identity) {
            Some(kept) => kept,
            None => new_person(connection).await?,
        }
    } else {
        match ranked.first() {
            Some((first, _)) => *first,
            None => new_person(connection).await?,
        }
    };
    let survivor_master = me.master
        || ranked
            .iter()
            .any(|(person, master)| *person == survivor && *master);
    let held: BTreeSet<PersonId> = ranked
        .iter()
        .filter(|(person, master)| *master && survivor_master && *person != survivor)
        .map(|(person, _)| *person)
        .collect();
    if current.is_none() {
        let kind = if joinable.is_empty() {
            LinkKind::Feed
        } else {
            LinkKind::Deterministic
        };
        open_link(connection, me.record, survivor, kind, None, context).await?;
    }
    for (person, _) in ranked
        .iter()
        .filter(|(person, _)| *person != survivor && !held.contains(person))
    {
        merge(
            connection,
            *person,
            survivor,
            MergeKind::Deterministic,
            context,
        )
        .await?;
        linked.merged.push(*person);
    }
    let masters = matches
        .iter()
        .filter(|other| held.contains(&other.person))
        .map(|other| (other, vec![Conflict::MasterIdentities]));
    for (other, found) in conflicting.into_iter().chain(masters) {
        let mut scored = score_pair(feeder, me, other);
        scored.evidence.conflicts = found;
        scored.grade = scored.grade.max(Grade::Possible);
        scored.evidence.grade = scored.grade;
        if let Some(id) = candidate(connection, me, other, &scored, context).await? {
            linked.candidates.push(id);
        }
    }
    Ok(survivor)
}

/// The probabilistic tier: scores `me` against every record in another
/// person that shares a blocking key with it, records each pair graded
/// `possible` or better as a candidate, or with `auto_link` merges the
/// persons of a `certain` pair, and returns the person `me` is in.
async fn probabilistic(
    connection: &mut PgConnection,
    feeder: &Feeder,
    me: &Me<'_>,
    survivor: PersonId,
    context: &Context,
    linked: &mut Linked,
) -> Result<PersonId, FeedError> {
    let mut survivor = survivor;
    let mut merged_away: BTreeSet<PersonId> = BTreeSet::new();
    for other in &blocked(connection, feeder, me, survivor).await? {
        if other.person == survivor || merged_away.contains(&other.person) {
            continue;
        }
        let mut scored = score_pair(feeder, me, other);
        scored.evidence.blocking_key = other_key(feeder, me, other);
        linked.scored = linked.scored.saturating_add(1);
        if scored.grade == Grade::CertainlyNot {
            continue;
        }
        let mut pair = [other.person, survivor];
        pair.sort_unstable();
        let ranked = by_survival(connection, &pair).await?;
        if ranked.iter().all(|(_, master)| *master) {
            scored.evidence.conflicts = vec![Conflict::MasterIdentities];
            if let Some(id) = candidate(connection, me, other, &scored, context).await? {
                linked.candidates.push(id);
            }
            continue;
        }
        if scored.grade == Grade::Certain && feeder.auto_link {
            lock_persons(connection, &pair).await?;
            let ranked = by_survival(connection, &pair).await?;
            let (Some((older, _)), Some((younger, _))) =
                (ranked.first().copied(), ranked.get(1).copied())
            else {
                return Err(FeedError::Contention);
            };
            let recording = Recording {
                context,
                kind: "auto-link",
                candidate: None,
                reason: None,
            };
            merge_moving(
                connection,
                (younger, older),
                MergeKind::Probabilistic,
                recording,
                Moved::Scored(&scored),
            )
            .await?;
            linked.merged.push(younger);
            merged_away.insert(younger);
            survivor = older;
            continue;
        }
        if let Some(id) = candidate(connection, me, other, &scored, context).await? {
            linked.candidates.push(id);
        }
    }
    Ok(survivor)
}

/// Returns `persons` in the order they survive one another, each with
/// whether it is a master identity: a master identity first, then the one
/// created first, the smaller id breaking a tie. The id's format orders
/// nothing, since a Source may assign a master identity's id.
pub(super) async fn by_survival(
    connection: &mut PgConnection,
    persons: &[PersonId],
) -> Result<Vec<(PersonId, bool)>, FeedError> {
    let ids: Vec<Uuid> = persons.iter().map(|person| person.uuid()).collect();
    let rows: Vec<(Uuid, bool)> = sqlx::query_as(
        "SELECT p.id, EXISTS (
             SELECT 1 FROM pix.link l
             JOIN pix.source_record r ON r.id = l.record_id
             JOIN pix.identifier_domain d ON d.id = r.domain_id
             WHERE l.person_id = p.id AND upper_inf(l.valid) AND d.kind = 'master'
         ) AS master
         FROM pix.person p WHERE p.id = ANY($1)
         ORDER BY master DESC, p.created_at, p.id",
    )
    .bind(&ids)
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    Ok(rows
        .into_iter()
        .map(|(id, master)| (PersonId::from_uuid(id), master))
        .collect())
}

/// Takes the advisory lock of each of `persons` in key order and refuses
/// with [`FeedError::Contention`] when one is no longer active.
pub(super) async fn lock_persons(
    connection: &mut PgConnection,
    persons: &[PersonId],
) -> Result<(), FeedError> {
    write::advisory(
        connection,
        persons.iter().map(|p| write::person_key(*p)).collect(),
    )
    .await?;
    if all_active(connection, persons).await? {
        Ok(())
    } else {
        Err(FeedError::Contention)
    }
}

/// Scores `me` against `other`, in the candidate's order: the record with
/// the lower id is `a`.
fn score_pair(feeder: &Feeder, me: &Me<'_>, other: &Other) -> Scored {
    if me.record < other.record {
        feeder.scorer.score(me.subject, &other.subject, None)
    } else {
        feeder.scorer.score(&other.subject, me.subject, None)
    }
}

/// Returns the first blocking key, in the configured order, that `me` and
/// `other` share.
fn other_key(feeder: &Feeder, me: &Me<'_>, other: &Other) -> Option<BlockingKey> {
    feeder.blocking.iter().copied().find(|key| match key {
        BlockingKey::SharedIdentifier => me
            .local
            .iter()
            .any(|k| other.subject.local_identifiers.contains(k)),
        text => {
            let mine = crate::matching::blocking::text_key(*text, me.subject);
            mine.is_some() && mine == crate::matching::blocking::text_key(*text, &other.subject)
        }
    })
}

/// Returns the person `record`'s open link names.
pub(super) async fn open_person(
    connection: &mut PgConnection,
    record: RecordId,
) -> Result<Option<PersonId>, FeedError> {
    let person: Option<Uuid> = sqlx::query_scalar(
        "SELECT person_id FROM pix.link WHERE record_id = $1 AND upper_inf(valid)",
    )
    .bind(record.uuid())
    .fetch_optional(&mut *connection)
    .await
    .map_err(store)?;
    Ok(person.map(PersonId::from_uuid))
}

/// Returns whether every one of `persons` is still active.
async fn all_active(
    connection: &mut PgConnection,
    persons: &[PersonId],
) -> Result<bool, FeedError> {
    let ids: Vec<Uuid> = persons.iter().map(|p| p.uuid()).collect();
    let inactive: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pix.person WHERE id = ANY($1) AND status <> 'active'",
    )
    .bind(&ids)
    .fetch_one(&mut *connection)
    .await
    .map_err(store)?;
    Ok(inactive == 0)
}

/// Inserts a new person and returns it.
async fn new_person(connection: &mut PgConnection) -> Result<PersonId, FeedError> {
    let id: Uuid = sqlx::query_scalar("INSERT INTO pix.person DEFAULT VALUES RETURNING id")
        .fetch_one(&mut *connection)
        .await
        .map_err(store)?;
    Ok(PersonId::from_uuid(id))
}

/// Opens a link of `record` to `person` and records the system action.
pub(super) async fn open_link(
    connection: &mut PgConnection,
    record: RecordId,
    person: PersonId,
    kind: LinkKind,
    score: Option<f64>,
    context: &Context,
) -> Result<(), FeedError> {
    sqlx::query(
        "INSERT INTO pix.link (record_id, person_id, kind, score, valid, made_by)
         VALUES ($1, $2, $3, $4, tstzrange(clock_timestamp(), NULL), $5)",
    )
    .bind(record.uuid())
    .bind(person.uuid())
    .bind(kind.as_str())
    .bind(score)
    .bind(&context.actor)
    .execute(&mut *connection)
    .await
    .map_err(store)?;
    write::action(
        connection,
        "link",
        context,
        Subjects {
            person: Some(person),
            record: Some(record),
            ..Subjects::default()
        },
    )
    .await?;
    Ok(())
}

/// Closes `record`'s open link, ended by the action `action`, and returns
/// the person it named.
pub(super) async fn close_link(
    connection: &mut PgConnection,
    record: RecordId,
    action: Uuid,
) -> Result<Option<PersonId>, FeedError> {
    let person: Option<Uuid> = sqlx::query_scalar(
        "UPDATE pix.link SET valid = tstzrange(lower(valid), clock_timestamp()), ended_by_action = $2
         WHERE record_id = $1 AND upper_inf(valid)
         RETURNING person_id",
    )
    .bind(record.uuid())
    .bind(action)
    .fetch_optional(&mut *connection)
    .await
    .map_err(store)?;
    Ok(person.map(PersonId::from_uuid))
}

/// How a merge is recorded: the system action of a feed, or a steward's
/// decision with its kind, its candidate and its reason.
#[derive(Debug, Clone, Copy)]
pub(super) struct Recording<'a> {
    /// The actor, the channel and the request.
    pub context: &'a Context,
    /// The steward action's kind.
    pub kind: &'static str,
    /// The candidate the decision settles.
    pub candidate: Option<Uuid>,
    /// The reason the steward gave.
    pub reason: Option<&'a str>,
}

/// What a merge wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Merged {
    /// The merge row.
    pub merge: Uuid,
    /// The steward action that records it.
    pub action: Uuid,
}

/// Merges `merged` into `survivor` as a system action of the feed.
pub(super) async fn merge(
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
enum Moved<'a> {
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
pub(super) async fn merge_recorded(
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
async fn merge_moving(
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
    // TODO(#104): queue the PMIR merge message for each matching subscription.
    Ok(Merged { merge, action })
}

/// Closes as linked every open or deferred candidate whose two records are
/// both in `survivor`, decided by `actor`.
async fn close_candidates(
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
async fn candidate(
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
async fn decided_apart(
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

/// The records in another person whose current version has the phonetic
/// blocking key `$1`.
const BY_PHONETIC: &str = "SELECT r.id, r.head_version, l.person_id, v.demographics::text
     FROM pix.source_record_version v
     JOIN pix.source_record r ON r.id = v.record_id AND v.version = r.head_version
     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
     WHERE v.block_phonetic = $1 AND r.id <> $2 AND l.person_id <> $3
       AND r.active AND r.removed_at IS NULL";

/// The records in another person whose current version has the postal
/// blocking key `$1`.
const BY_POSTAL: &str = "SELECT r.id, r.head_version, l.person_id, v.demographics::text
     FROM pix.source_record_version v
     JOIN pix.source_record r ON r.id = v.record_id AND v.version = r.head_version
     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
     WHERE v.block_postal = $1 AND r.id <> $2 AND l.person_id <> $3
       AND r.active AND r.removed_at IS NULL";

/// A row of another record: id, current version, person, demographics.
type OtherRow = (Uuid, i32, Uuid, String);

/// Returns the records whose current version carries one of `me`'s
/// authoritative identifiers.
async fn deterministic(
    connection: &mut PgConnection,
    me: &Me<'_>,
) -> Result<Vec<Other>, FeedError> {
    if me.authoritative.is_empty() {
        return Ok(Vec::new());
    }
    let (domains, hmacs) = split(&me.authoritative);
    let rows: Vec<OtherRow> = sqlx::query_as(
        "SELECT DISTINCT r.id, r.head_version, l.person_id, v.demographics::text
         FROM pix.carried_identifier i
         JOIN pix.source_record r ON r.id = i.record_id AND i.version = r.head_version
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
         JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
         WHERE (i.domain_id, i.identifier_hmac) IN (SELECT * FROM unnest($1::uuid[], $2::bytea[]))
           AND r.id <> $3 AND r.active AND r.removed_at IS NULL
         ORDER BY r.id",
    )
    .bind(&domains)
    .bind(&hmacs)
    .bind(me.record.uuid())
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    others(connection, rows).await
}

/// Returns the records in another person than `survivor` that share one of
/// the configured blocking keys with `me`, each once, ordered by id.
async fn blocked(
    connection: &mut PgConnection,
    feeder: &Feeder,
    me: &Me<'_>,
    survivor: PersonId,
) -> Result<Vec<Other>, FeedError> {
    let mut rows: BTreeMap<Uuid, OtherRow> = BTreeMap::new();
    for key in &feeder.blocking {
        let found: Vec<OtherRow> = match key {
            BlockingKey::BirthYearFamilyPhonetic | BlockingKey::PostalCodeBirthDate => {
                let (statement, value) = if *key == BlockingKey::BirthYearFamilyPhonetic {
                    (BY_PHONETIC, me.phonetic.as_deref())
                } else {
                    (BY_POSTAL, me.postal.as_deref())
                };
                let Some(value) = value else {
                    continue;
                };
                sqlx::query_as(statement)
                    .bind(value)
                    .bind(me.record.uuid())
                    .bind(survivor.uuid())
                    .fetch_all(&mut *connection)
                    .await
                    .map_err(store)?
            }
            BlockingKey::SharedIdentifier => {
                if me.local.is_empty() {
                    continue;
                }
                let (domains, hmacs) = split(&me.local);
                sqlx::query_as(
                    "SELECT DISTINCT r.id, r.head_version, l.person_id, v.demographics::text
                     FROM pix.carried_identifier i
                     JOIN pix.source_record r ON r.id = i.record_id AND i.version = r.head_version
                     JOIN pix.source_record_version v
                          ON v.record_id = r.id AND v.version = r.head_version
                     JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
                     WHERE (i.domain_id, i.identifier_hmac)
                               IN (SELECT * FROM unnest($1::uuid[], $2::bytea[]))
                       AND r.id <> $3 AND l.person_id <> $4
                       AND r.active AND r.removed_at IS NULL",
                )
                .bind(&domains)
                .bind(&hmacs)
                .bind(me.record.uuid())
                .bind(survivor.uuid())
                .fetch_all(&mut *connection)
                .await
                .map_err(store)?
            }
        };
        for row in found {
            rows.entry(row.0).or_insert(row);
        }
    }
    others(connection, rows.into_values().collect()).await
}

/// Reads each row's demographics and the identifiers its version carries in
/// `local` domains into an [`Other`].
async fn others(
    connection: &mut PgConnection,
    rows: Vec<OtherRow>,
) -> Result<Vec<Other>, FeedError> {
    let ids: Vec<Uuid> = rows.iter().map(|row| row.0).collect();
    let local: Vec<(Uuid, Uuid, Vec<u8>)> = sqlx::query_as(
        "SELECT i.record_id, i.domain_id, i.identifier_hmac
         FROM pix.carried_identifier i
         JOIN pix.source_record r ON r.id = i.record_id AND i.version = r.head_version
         JOIN pix.identifier_domain d ON d.id = i.domain_id
         WHERE i.record_id = ANY($1) AND d.kind = $2",
    )
    .bind(&ids)
    .bind(crate::store::seed::kind_name(DomainKind::Local))
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    let mut keys: BTreeMap<Uuid, BTreeSet<IdentifierKey>> = BTreeMap::new();
    for (record, domain, hmac) in local {
        let hmac: [u8; 32] = hmac.try_into().map_err(|_short| {
            FeedError::Store(crate::store::StoreError::Seal(
                crate::store::seal::SealError::Malformed,
            ))
        })?;
        keys.entry(record)
            .or_default()
            .insert(IdentifierKey::new(domain, hmac));
    }
    rows.into_iter()
        .map(|(record, version, person, text)| {
            let patient = demographics::patient(&text)?;
            let local = keys.remove(&record).unwrap_or_default();
            Ok(Other {
                record: RecordId::from_uuid(record),
                version,
                person: PersonId::from_uuid(person),
                subject: Subject::from_patient(&patient, local),
            })
        })
        .collect()
}

/// Splits `keys` into the domain and HMAC arrays a statement binds.
fn split(keys: &[IdentifierKey]) -> (Vec<Uuid>, Vec<Vec<u8>>) {
    keys.iter()
        .map(|key| (key.domain(), key.hmac().to_vec()))
        .unzip()
}

/// Wraps a driver error.
fn store(source: sqlx::Error) -> FeedError {
    FeedError::Store(source.into())
}
