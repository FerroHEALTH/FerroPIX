// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The feed transaction: what an ITI-104 message does to the store, as one
//! PostgreSQL transaction with explicit locks (`docs/architecture.md` §4.3).
//!
//! - **Add or revise** (PIXm 3.1.0 §2:3.104.4.1): the record is locked by
//!   `(domain, hmac)` or inserted, a version written with its demographics
//!   (no identifier among them) and the identifiers it carries, and the
//!   cross-referencing run over it: the deterministic match on
//!   `authoritative` identifiers, the persons it names locked in key order
//!   and joined, and the probabilistic candidates from the blocking keys.
//! - **Resolve duplicate** (§2:3.104.4.2.3): both records locked, the
//!   subsumed one marked `replaced_by` the survivor and its link closed, its
//!   person merged into the survivor's with `kind = source`, and the
//!   cross-referencing run again over the survivor.
//! - **Remove** (§2:3.104.4.3.3): the record locked, `removed_at` set and
//!   its link closed; its versions stay.
//!
//! Every identifier a transaction will lock is taken as an advisory lock
//! first, in key order, then the record rows, then the persons in key
//! order, so two feeds never deadlock on each other and a feed that joins
//! the persons another feed is joining waits for it. Every system action is
//! a `steward_action` row. The BALP record is inserted into the audit
//! outbox in the same transaction, before it commits ([`Recording`]). PIXm leaves the cross-referencing to the Manager
//! (§2:3.104.4.1.3): the rules are our own design.

pub mod announce;
mod attempt;
mod link;
pub mod master;
pub mod merging;
pub mod notify;
pub mod steward;
mod write;

use fhir_types::r4::patient::Patient;
use ihe_atna::balp::{Exchange, Outcome};
use ihe_iti::pixm::audit::{FeedAction, manager_feed};
use ihe_iti::pixm::identifier::SourceIdentifier;
use ihe_iti::pixm::server::feed::{Merge, Removal, Revision};
use secrecy::ExposeSecret as _;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::audit::records::Call;
use crate::audit::{Audit, Unrecorded};
use crate::config::section::domain::DomainKind;
use crate::config::section::matching::{BlockingKey, Matching};
use crate::feed::attempt::{
    Resolving, Revising, announced, condition, current, identity_keys, local_keys, me,
};
use crate::feed::link::MergeKind;
use crate::feed::write::{Carried, Subjects};
use crate::matching::scorer::Scorer;
use crate::matching::subject::Subject;
use crate::store::demographics::{self, DemographicsError};
use crate::store::identifier::Refused;
use crate::store::record::{self, Blocks, Channel, PersonId, RecordId};
use crate::store::{Store, StoreError};

/// The times a transaction is tried when a concurrent one moved what it
/// read or the database broke a deadlock.
pub const ATTEMPTS: usize = 3;

/// The id of a candidate in the review queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CandidateId(Uuid);

impl CandidateId {
    /// Returns the uuid.
    #[must_use]
    pub fn uuid(self) -> Uuid {
        self.0
    }
}

/// Who sent a message, on which channel, under which request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// The client id of the domain's Source, recorded as the actor of every
    /// system action.
    pub actor: String,
    /// The channel the message arrived on.
    pub channel: Channel,
    /// The request id, recorded beside each version and action.
    pub request_id: Option<String>,
}

/// What a message did to its record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The identifier was new: its record was created.
    Create,
    /// The identifier was known: its record was revised.
    Update,
    /// The Source resolved a duplicate: the record was subsumed.
    ResolveDuplicate,
    /// The identifier was removed.
    Remove,
}

impl Action {
    /// Returns what the ITI-104 audit record calls the action (PIXm 3.1.0
    /// §2:3.104.5.1.2): a resolved duplicate is an update of the subsumed
    /// identifier.
    #[must_use]
    pub const fn audited(self) -> FeedAction {
        match self {
            Self::Create => FeedAction::Create,
            Self::Update | Self::ResolveDuplicate => FeedAction::Update,
            Self::Remove => FeedAction::Delete,
        }
    }
}

/// What one committed message did.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// What it did to its record.
    pub action: Action,
    /// The record the message named.
    pub record: RecordId,
    /// The person the record, or the survivor of a resolved duplicate, is
    /// in afterwards; `None` after a removal.
    pub person: Option<PersonId>,
    /// The persons merged into [`Applied::person`].
    pub merged: Vec<PersonId>,
    /// The candidates recorded or refreshed.
    pub candidates: Vec<CandidateId>,
    /// The pairs the probabilistic tier scored.
    pub scored: usize,
}

/// The feed transaction over one store and one `[matching]` section.
#[derive(Debug, Clone)]
pub struct Feeder {
    /// The store.
    store: Store,
    /// The scorer.
    scorer: Scorer,
    /// Whether a `certain` pair is linked without a steward.
    auto_link: bool,
    /// The blocking keys, in the configured order.
    blocking: Vec<BlockingKey>,
}

impl Feeder {
    /// Returns the feed transaction over `store` with the `[matching]`
    /// section `matching`, which [`Matching::resolve`] accepted.
    #[must_use]
    pub fn new(store: Store, matching: &Matching) -> Self {
        Self {
            store,
            scorer: Scorer::new(matching),
            auto_link: matching.auto_link,
            blocking: matching.blocking_keys.clone(),
        }
    }

    /// Applies an Add or Revise Patient message (PIXm 3.1.0 §2:3.104.4.1),
    /// its records inserted in its transaction before it commits.
    ///
    /// # Errors
    /// Returns [`FeedError::UnknownDomain`] for a condition in a domain the
    /// store does not know or retired, [`FeedError::Identifier`] for a value
    /// its domain's rule refuses, [`FeedError::Subsumed`] for an identifier
    /// the Source subsumed, [`FeedError::Contention`] when concurrent
    /// transactions moved what it read on every attempt,
    /// [`FeedError::Unrecorded`] when its records cannot be kept, and
    /// [`FeedError::Store`] when a statement fails; nothing is committed.
    pub async fn revise(
        &self,
        revision: &Revision,
        context: &Context,
        recording: Recording<'_>,
    ) -> Result<Applied, FeedError> {
        let revising = Revising {
            system: revision.identifier().system(),
            value: revision.identifier().value().expose_secret(),
            master: None,
        };
        self.apply(
            Step::Revise(revising, revision.patient()),
            context,
            recording,
        )
        .await
    }

    /// Applies a Resolve Duplicate Patient message (PIXm 3.1.0
    /// §2:3.104.4.2), its records inserted in its transaction.
    ///
    /// # Errors
    /// As [`Feeder::revise`], and [`FeedError::SurvivorNotFound`] when the
    /// surviving identifier names no active record of the domain.
    pub async fn resolve_duplicate(
        &self,
        merge: &Merge,
        context: &Context,
        recording: Recording<'_>,
    ) -> Result<Applied, FeedError> {
        let resolving = Resolving {
            system: merge.subsumed().system(),
            subsumed: merge.subsumed().value().expose_secret(),
            survivor: merge.survivor().value().expose_secret(),
        };
        self.apply(
            Step::Resolve(resolving, merge.patient()),
            context,
            recording,
        )
        .await
    }

    /// Applies a Remove Patient message (PIXm 3.1.0 §2:3.104.4.3), its
    /// records inserted in its transaction.
    ///
    /// # Errors
    /// Returns [`FeedError::UnknownDomain`] and [`FeedError::Identifier`] as
    /// [`Feeder::revise`] does, [`FeedError::NotFound`] for an identifier
    /// the store does not hold or removed already, [`FeedError::Unrecorded`]
    /// when its records cannot be kept, and [`FeedError::Store`] when a
    /// statement fails.
    pub async fn remove(
        &self,
        removal: &Removal,
        context: &Context,
        recording: Recording<'_>,
    ) -> Result<Applied, FeedError> {
        let identifier = removal.identifier();
        let step = Step::Remove {
            system: identifier.system(),
            value: identifier.value().expose_secret(),
            deleted: false,
        };
        self.apply(step, context, recording).await
    }

    /// Applies `step` in one transaction that holds its records too, tried
    /// again after contention. A step refused before it committed has its
    /// records inserted in a statement of their own: it changed nothing.
    async fn apply(
        &self,
        step: Step<'_>,
        context: &Context,
        recording: Recording<'_>,
    ) -> Result<Applied, FeedError> {
        let mut attempt = 0;
        let outcome = loop {
            attempt += 1;
            match self.apply_once(step, context, recording).await {
                Err(error) if error.retryable() && attempt < ATTEMPTS => {
                    tracing::debug!(attempt, "a feed is tried again after contention");
                }
                outcome => break outcome,
            }
        };
        match outcome {
            Ok(applied) => {
                self.store.announced();
                recording.audit.committed();
                tracing::info!(
                    action = ?applied.action,
                    record = %applied.record.uuid(),
                    merged = applied.merged.len(),
                    candidates = applied.candidates.len(),
                    scored = applied.scored,
                    "a feed is applied"
                );
                Ok(applied)
            }
            Err(error @ FeedError::Unrecorded(_)) => Err(error),
            Err(error) => {
                recording.refused(&error).await?;
                Err(error)
            }
        }
    }

    /// One attempt at [`Feeder::apply`]: the step and its records, then the
    /// commit.
    async fn apply_once(
        &self,
        step: Step<'_>,
        context: &Context,
        recording: Recording<'_>,
    ) -> Result<Applied, FeedError> {
        let mut transaction = self.store.pool().begin().await.map_err(store)?;
        let applied = self.step_in(&mut transaction, step, context).await?;
        recording.enlist(&mut transaction, Ok(&applied)).await?;
        transaction.commit().await.map_err(store)?;
        Ok(applied)
    }

    /// Writes `step` in `transaction`, which the caller commits.
    async fn step_in(
        &self,
        transaction: &mut PgConnection,
        step: Step<'_>,
        context: &Context,
    ) -> Result<Applied, FeedError> {
        match step {
            Step::Revise(revising, patient) => {
                self.revise_in(transaction, &revising, patient, context)
                    .await
            }
            Step::Resolve(resolving, patient) => {
                self.resolve_in(transaction, &resolving, patient, context)
                    .await
            }
            Step::Remove {
                system,
                value,
                deleted,
            } => {
                self.remove_in(transaction, system, value, deleted, context)
                    .await
            }
        }
    }

    /// Removes the identifier `value` of the domain `system` in
    /// `transaction`, announced as the deletion of its person's master
    /// identity when `deleted`.
    async fn remove_in(
        &self,
        transaction: &mut PgConnection,
        system: &str,
        value: &str,
        deleted: bool,
        context: &Context,
    ) -> Result<Applied, FeedError> {
        let keys = self.store.sealer();
        let domains = record::domains(&mut *transaction).await?;
        let (domain, value) = condition(&domains, system, value)?;
        let hmac = keys.hmac(domain.id.uuid(), &value);
        write::advisory(&mut *transaction, vec![record::identity_key(&hmac)]).await?;
        let Some(locked) = write::lock(&mut *transaction, domain.id, &hmac).await? else {
            return Err(FeedError::NotFound);
        };
        if locked.removed {
            return Err(FeedError::NotFound);
        }
        let action = write::recorded(
            &mut *transaction,
            "remove",
            context,
            Subjects {
                record: Some(locked.id),
                ..Subjects::default()
            },
            None,
        )
        .await?;
        sqlx::query("UPDATE pix.source_record SET removed_at = now() WHERE id = $1")
            .bind(locked.id.uuid())
            .execute(&mut *transaction)
            .await
            .map_err(store)?;
        let former = link::close_link(&mut *transaction, locked.id, action).await?;
        let changes: Vec<announce::Change> = former
            .map(|person| {
                if deleted {
                    announce::Change::Deleted(person)
                } else {
                    announce::Change::Updated(person)
                }
            })
            .into_iter()
            .collect();
        announce::announce(&mut *transaction, &self.store, &changes).await?;
        Ok(Applied {
            action: Action::Remove,
            record: locked.id,
            person: None,
            merged: Vec::new(),
            candidates: Vec::new(),
            scored: 0,
        })
    }

    /// Writes the revision `revising` with `patient` in `transaction`.
    async fn revise_in(
        &self,
        transaction: &mut PgConnection,
        revising: &Revising<'_>,
        patient: &Patient,
        context: &Context,
    ) -> Result<Applied, FeedError> {
        let keys = self.store.sealer();
        let domains = record::domains(&mut *transaction).await?;
        let (domain, value) = condition(&domains, revising.system, revising.value)?;
        let carried = write::carried(patient, &domains, keys)?;
        let own = keys.hmac(domain.id.uuid(), &value);
        write::advisory(&mut *transaction, identity_keys(&own, &carried)).await?;
        let fed = Fed::new(patient, &carried)?;
        let locked = match revising.master {
            Some(master) if master.known => write::lock(&mut *transaction, domain.id, &own)
                .await?
                .filter(|locked| !locked.removed)
                .ok_or(FeedError::NotFound)?,
            _ => {
                write::lock_or_insert(&mut *transaction, keys, domain, &value, &fed.blocks).await?
            }
        };
        if !locked.active {
            return Err(FeedError::Subsumed);
        }
        if let Some(master) = revising.master
            && master::merged(&mut *transaction, master.identity).await?
        {
            return Err(FeedError::Subsumed);
        }
        let version = self
            .write_version(&mut *transaction, &locked, &fed, context)
            .await?;
        write::recorded(
            &mut *transaction,
            "feed",
            context,
            Subjects {
                record: Some(locked.id),
                ..Subjects::default()
            },
            None,
        )
        .await?;
        let mut me = me(
            locked.id,
            version,
            &fed.subject,
            &carried,
            domain.kind == DomainKind::Master,
        );
        me.identity = revising.master.map(|master| master.identity);
        let linked = link::cross_reference(&mut *transaction, self, &me, context).await?;
        let created = revising.master.is_some_and(|master| !master.known) && locked.created;
        let changes = announced(linked.person, &linked.merged, created);
        announce::announce(&mut *transaction, &self.store, &changes).await?;
        let action = if locked.created {
            Action::Create
        } else {
            Action::Update
        };
        Ok(Applied {
            action,
            record: locked.id,
            person: linked.person,
            merged: linked.merged,
            candidates: linked.candidates,
            scored: linked.scored,
        })
    }

    /// Writes the resolved duplicate `resolving` with `patient` in
    /// `transaction`.
    async fn resolve_in(
        &self,
        transaction: &mut PgConnection,
        resolving: &Resolving<'_>,
        patient: &Patient,
        context: &Context,
    ) -> Result<Applied, FeedError> {
        let keys = self.store.sealer();
        let domains = record::domains(&mut *transaction).await?;
        let (domain, subsumed) = condition(&domains, resolving.system, resolving.subsumed)?;
        let (_, survivor) = condition(&domains, resolving.system, resolving.survivor)?;
        let carried = write::carried(patient, &domains, keys)?;
        let subsumed_hmac = keys.hmac(domain.id.uuid(), &subsumed);
        let survivor_hmac = keys.hmac(domain.id.uuid(), &survivor);
        let mut identities = identity_keys(&subsumed_hmac, &carried);
        identities.push(record::identity_key(&survivor_hmac));
        write::advisory(&mut *transaction, identities).await?;
        let Some(kept) = write::lock(&mut *transaction, domain.id, &survivor_hmac).await? else {
            return Err(FeedError::SurvivorNotFound);
        };
        if !kept.active || kept.removed {
            return Err(FeedError::SurvivorNotFound);
        }
        let fed = Fed::new(patient, &carried)?;
        let locked =
            write::lock_or_insert(&mut *transaction, keys, domain, &subsumed, &fed.blocks).await?;
        self.write_version(&mut *transaction, &locked, &fed, context)
            .await?;
        let action = write::recorded(
            &mut *transaction,
            "resolve-duplicate",
            context,
            Subjects {
                record: Some(locked.id),
                ..Subjects::default()
            },
            None,
        )
        .await?;
        sqlx::query("UPDATE pix.source_record SET active = false, replaced_by = $2 WHERE id = $1")
            .bind(locked.id.uuid())
            .bind(kept.id.uuid())
            .execute(&mut *transaction)
            .await
            .map_err(store)?;
        let former = link::close_link(&mut *transaction, locked.id, action).await?;
        let (subject, survivor_carried, version) =
            current(&mut *transaction, keys, kept.id, &domains).await?;
        write::advisory(
            &mut *transaction,
            survivor_carried
                .iter()
                .filter(|c| c.kind == DomainKind::Authoritative)
                .map(|c| record::identity_key(&c.hmac))
                .collect(),
        )
        .await?;
        let held = sqlx::query_scalar::<_, Uuid>(
            "SELECT person_id FROM pix.link WHERE record_id = $1 AND upper_inf(valid)",
        )
        .bind(kept.id.uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(store)?
        .map(PersonId::from_uuid);
        let mut merged = Vec::new();
        if let (Some(former), Some(held)) = (former, held)
            && former != held
        {
            write::advisory(
                &mut *transaction,
                vec![write::person_key(former), write::person_key(held)],
            )
            .await?;
            link::merge::merge(&mut *transaction, former, held, MergeKind::Source, context).await?;
            merged.push(former);
        }
        let me = me(
            kept.id,
            version,
            &subject,
            &survivor_carried,
            domain.kind == DomainKind::Master,
        );
        let linked = link::cross_reference(&mut *transaction, self, &me, context).await?;
        merged.extend(linked.merged);
        let changes = announced(linked.person, &merged, false);
        announce::announce(&mut *transaction, &self.store, &changes).await?;
        Ok(Applied {
            action: Action::ResolveDuplicate,
            record: locked.id,
            person: linked.person,
            merged,
            candidates: linked.candidates,
            scored: linked.scored,
        })
    }

    /// Writes the next version of `locked` from `fed`, with its blocking
    /// keys and the identifiers it carries, and returns its number.
    async fn write_version(
        &self,
        connection: &mut PgConnection,
        locked: &write::Locked,
        fed: &Fed<'_>,
        context: &Context,
    ) -> Result<i32, FeedError> {
        let stored = demographics::stored(fed.patient)?;
        let version = write::add_version(connection, locked, &stored, &fed.blocks, context).await?;
        let sealer = self.store.sealer();
        write::insert_carried(connection, sealer, locked.id, version, fed.carried).await?;
        Ok(version)
    }
}

/// The `Patient` of a feed with the identifiers it carries, the subject the
/// matching weighs, and the blocking keys and scoring features its version
/// takes.
struct Fed<'a> {
    patient: &'a Patient,
    carried: &'a [Carried],
    subject: Subject,
    blocks: Blocks,
}

impl<'a> Fed<'a> {
    /// Reads `patient` with the identifiers it carries.
    fn new(patient: &'a Patient, carried: &'a [Carried]) -> Result<Self, StoreError> {
        let subject = Subject::from_patient(patient, local_keys(carried));
        let blocks = Blocks::of(&subject)?;
        Ok(Self {
            patient,
            carried,
            subject,
            blocks,
        })
    }
}

/// Returns the BALP record of one ITI-104 message: the PIXm Feed Create,
/// Update or Delete Manager audit profile (PIXm 3.1.0 §2:3.104.5.1.2) of
/// `identifier`, the condition, with the person as the data.
///
/// A message refused before it applied is recorded as an update or a
/// delete that failed.
#[must_use]
pub fn feed_record(
    call: &Call,
    identifier: &SourceIdentifier,
    removal: bool,
    outcome: Result<&Applied, &FeedError>,
) -> Exchange {
    let (action, person, outcome) = match outcome {
        Ok(applied) => (
            applied.action.audited(),
            applied.person.map(|p| format!("Patient/{}", p.uuid())),
            Outcome::Success,
        ),
        Err(_) if removal => (FeedAction::Delete, None, Outcome::MinorFailure),
        Err(_) => (FeedAction::Update, None, Outcome::MinorFailure),
    };
    manager_feed(
        &call.received(),
        action,
        identifier,
        person.as_deref(),
        outcome,
    )
}

/// How a feed records what it did (#742).
///
/// The records of its outcome are inserted into the audit outbox in its
/// transaction before it commits, or, for a feed refused before it
/// committed, in a statement of their own.
#[derive(Clone, Copy)]
pub struct Recording<'a> {
    /// The audit trail.
    audit: &'a Audit,
    /// The records of an outcome.
    records: &'a (dyn Fn(Result<&Applied, &FeedError>) -> Vec<Exchange> + Sync),
}

impl std::fmt::Debug for Recording<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recording").finish_non_exhaustive()
    }
}

impl<'a> Recording<'a> {
    /// Records each outcome as `records` writes it, on `audit`.
    #[must_use]
    pub fn new(
        audit: &'a Audit,
        records: &'a (dyn Fn(Result<&Applied, &FeedError>) -> Vec<Exchange> + Sync),
    ) -> Self {
        Self { audit, records }
    }

    /// Inserts the records of `outcome` through `connection`.
    async fn enlist(
        &self,
        connection: &mut PgConnection,
        outcome: Result<&Applied, &FeedError>,
    ) -> Result<(), FeedError> {
        let exchanges = (self.records)(outcome);
        self.audit
            .enlist(connection, &exchanges)
            .await
            .map_err(FeedError::Unrecorded)
    }

    /// Keeps the records of the refusal `error` in a statement of their
    /// own: the feed changed nothing.
    async fn refused(&self, error: &FeedError) -> Result<(), FeedError> {
        let exchanges = (self.records)(Err(error));
        self.audit
            .keep(&exchanges)
            .await
            .map_err(FeedError::Unrecorded)
    }
}

/// One change the feed transaction writes.
#[derive(Debug, Clone, Copy)]
enum Step<'a> {
    /// An add or revise, of a feed or of an ITI-93 master identity.
    Revise(Revising<'a>, &'a Patient),
    /// A resolved duplicate, or an ITI-93 merge.
    Resolve(Resolving<'a>, &'a Patient),
    /// A removal, announced as the deletion of the person's master identity
    /// when `deleted`.
    Remove {
        /// The domain's system.
        system: &'a str,
        /// The identifier's value.
        value: &'a str,
        /// Whether an ITI-93 delete removes it.
        deleted: bool,
    },
}

/// Why a message was not applied.
///
/// No variant carries an identifier value or a demographic.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FeedError {
    /// The condition names a domain the store does not know, or retired.
    #[error("the identifier's domain is not known to the store")]
    UnknownDomain,
    /// An identifier's value is refused by its domain's rule.
    #[error("an identifier of the domain {system} is refused by the domain's rule")]
    Identifier {
        /// The domain's system.
        system: String,
        /// Why the value is refused.
        #[source]
        source: Refused,
    },
    /// The condition names an identifier the Source subsumed.
    #[error("the identifier was subsumed by the Source and takes no new version")]
    Subsumed,
    /// The identifier is not held, or was removed.
    #[error("the identifier is not held by the store")]
    NotFound,
    /// The surviving identifier of a resolved duplicate names no active
    /// record of the domain.
    #[error("the surviving identifier names no active record of the domain")]
    SurvivorNotFound,
    /// The demographics do not read or write.
    #[error("the demographics cannot be stored")]
    Demographics(#[from] DemographicsError),
    /// The evidence of a candidate cannot be written.
    #[error("the evidence of a candidate cannot be written")]
    Evidence,
    /// The ITI-93 message announcing the change cannot be written.
    #[error("the ITI-93 message announcing the change cannot be written")]
    Notice,
    /// The ITI-10 notification of a consumer does not conform to its
    /// structure.
    #[error("the ITI-10 notification cannot be composed")]
    Notification(#[source] ferrobridge_hl7v2::write::ComposeError),
    /// Concurrent transactions moved what this one read.
    #[error("concurrent transactions moved the persons this feed joins")]
    Contention,
    /// A statement on the store failed.
    #[error("the store refused the feed")]
    Store(#[from] StoreError),
    /// The feed's audit records could not be kept, so it was not committed.
    #[error("the audit records of the feed could not be kept")]
    Unrecorded(#[source] Unrecorded),
}

impl FeedError {
    /// Returns whether the error refuses one change and leaves the store
    /// able to take the next: an identifier or a domain the change cannot
    /// name, or a record it cannot touch. Any other error ends the
    /// transaction.
    #[must_use]
    pub fn refuses_change(&self) -> bool {
        matches!(
            self,
            Self::UnknownDomain
                | Self::Identifier { .. }
                | Self::Subsumed
                | Self::NotFound
                | Self::SurvivorNotFound
        )
    }

    /// Returns whether another attempt may succeed: contention, a deadlock
    /// the database broke, or a serialization failure.
    #[must_use]
    pub fn retryable(&self) -> bool {
        match self {
            Self::Contention => true,
            Self::Store(error) => error
                .database()
                .and_then(crate::store::DbFailure::code)
                .is_some_and(|code| code == "40P01" || code == "40001"),
            _ => false,
        }
    }
}

/// Wraps a driver error.
fn store(source: sqlx::Error) -> FeedError {
    FeedError::Store(source.into())
}
