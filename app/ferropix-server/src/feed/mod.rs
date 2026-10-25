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
//! a `steward_action` row. The BALP record is written after the commit by
//! [`record()`]. PIXm leaves the cross-referencing to the Manager
//! (§2:3.104.4.1.3): the rules are our own design.

pub mod demographics;
mod link;
pub mod steward;
mod write;

use std::collections::BTreeSet;

use fhir_types::r4::patient::Patient;
use ihe_atna::balp::Outcome;
use ihe_iti::pixm::identifier::SourceIdentifier;
use ihe_iti::pixm::server::feed::{Merge, Removal, Revision};
use secrecy::ExposeSecret as _;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::audit::records::{self, Call, Identifier};
use crate::audit::reserve::Pending;
use crate::audit::{Audit, Unrecorded};
use crate::config::section::domain::DomainKind;
use crate::config::section::matching::{BlockingKey, Matching};
use crate::feed::demographics::DemographicsError;
use crate::feed::link::{Me, MergeKind};
use crate::feed::write::{Blocks, Carried, Subjects};
use crate::matching::blocking;
use crate::matching::scorer::Scorer;
use crate::matching::subject::{IdentifierKey, Subject};
use crate::store::identifier::{Refused, Rule};
use crate::store::record::{self, Channel, Domain, PersonId, RecordId};
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
    pub const fn audited(self) -> records::Feed {
        match self {
            Self::Create => records::Feed::Create,
            Self::Update | Self::ResolveDuplicate => records::Feed::Update,
            Self::Remove => records::Feed::Delete,
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

    /// Applies an Add or Revise Patient message (PIXm 3.1.0 §2:3.104.4.1).
    ///
    /// # Errors
    /// Returns [`FeedError::UnknownDomain`] for a condition in a domain the
    /// store does not know or retired, [`FeedError::Identifier`] for a value
    /// its domain's rule refuses, [`FeedError::Subsumed`] for an identifier
    /// the Source subsumed, [`FeedError::Contention`] when concurrent
    /// transactions moved what it read on every attempt, and
    /// [`FeedError::Store`] when a statement fails; nothing is committed.
    pub async fn revise(
        &self,
        revision: &Revision,
        context: &Context,
    ) -> Result<Applied, FeedError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.revise_once(revision, context).await {
                Err(error) if error.retryable() && attempt < ATTEMPTS => {
                    tracing::debug!(attempt, "a feed is tried again after contention");
                }
                outcome => return outcome,
            }
        }
    }

    /// Applies a Resolve Duplicate Patient message (PIXm 3.1.0
    /// §2:3.104.4.2).
    ///
    /// # Errors
    /// As [`Feeder::revise`], and [`FeedError::SurvivorNotFound`] when the
    /// surviving identifier names no active record of the domain.
    pub async fn resolve_duplicate(
        &self,
        merge: &Merge,
        context: &Context,
    ) -> Result<Applied, FeedError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.resolve_once(merge, context).await {
                Err(error) if error.retryable() && attempt < ATTEMPTS => {
                    tracing::debug!(attempt, "a feed is tried again after contention");
                }
                outcome => return outcome,
            }
        }
    }

    /// Applies a Remove Patient message (PIXm 3.1.0 §2:3.104.4.3).
    ///
    /// # Errors
    /// Returns [`FeedError::UnknownDomain`] and [`FeedError::Identifier`] as
    /// [`Feeder::revise`] does, [`FeedError::NotFound`] for an identifier
    /// the store does not hold or removed already, and
    /// [`FeedError::Store`] when a statement fails.
    pub async fn remove(&self, removal: &Removal, context: &Context) -> Result<Applied, FeedError> {
        let keys = self.store.sealer();
        let mut transaction = self.store.pool().begin().await.map_err(store)?;
        let domains = record::domains(&mut *transaction).await?;
        let (domain, value) = condition(&domains, removal.identifier())?;
        let hmac = keys.hmac(domain.id.uuid(), &value);
        write::advisory(&mut transaction, vec![write::identity_key(&hmac)]).await?;
        let Some(locked) = write::lock(&mut transaction, domain.id, &hmac).await? else {
            return Err(FeedError::NotFound);
        };
        if locked.removed {
            return Err(FeedError::NotFound);
        }
        let action = write::action(
            &mut transaction,
            "remove",
            context,
            Subjects {
                record: Some(locked.id),
                ..Subjects::default()
            },
        )
        .await?;
        sqlx::query("UPDATE pix.source_record SET removed_at = now() WHERE id = $1")
            .bind(locked.id.uuid())
            .execute(&mut *transaction)
            .await
            .map_err(store)?;
        link::close_link(&mut transaction, locked.id, action).await?;
        transaction.commit().await.map_err(store)?;
        tracing::info!(record = %locked.id.uuid(), "an identifier is removed");
        Ok(Applied {
            action: Action::Remove,
            record: locked.id,
            person: None,
            merged: Vec::new(),
            candidates: Vec::new(),
            scored: 0,
        })
    }

    /// One attempt at [`Feeder::revise`].
    async fn revise_once(
        &self,
        revision: &Revision,
        context: &Context,
    ) -> Result<Applied, FeedError> {
        let keys = self.store.sealer();
        let patient = revision.patient();
        let mut transaction = self.store.pool().begin().await.map_err(store)?;
        let domains = record::domains(&mut *transaction).await?;
        let (domain, value) = condition(&domains, revision.identifier())?;
        let carried = write::carried(patient, &domains, keys)?;
        let own = keys.hmac(domain.id.uuid(), &value);
        write::advisory(&mut transaction, identity_keys(&own, &carried)).await?;
        let locked = write::lock_or_insert(&mut transaction, keys, domain, &value).await?;
        if !locked.active {
            return Err(FeedError::Subsumed);
        }
        let (version, subject) = self
            .write_version(&mut transaction, &locked, patient, &carried, context)
            .await?;
        write::action(
            &mut transaction,
            "feed",
            context,
            Subjects {
                record: Some(locked.id),
                ..Subjects::default()
            },
        )
        .await?;
        let me = me(locked.id, version, &subject, &carried);
        let linked = link::cross_reference(&mut transaction, self, &me, context).await?;
        transaction.commit().await.map_err(store)?;
        let action = if locked.created {
            Action::Create
        } else {
            Action::Update
        };
        tracing::info!(
            record = %locked.id.uuid(),
            created = locked.created,
            merged = linked.merged.len(),
            candidates = linked.candidates.len(),
            scored = linked.scored,
            "a feed is applied"
        );
        Ok(Applied {
            action,
            record: locked.id,
            person: linked.person,
            merged: linked.merged,
            candidates: linked.candidates,
            scored: linked.scored,
        })
    }

    /// One attempt at [`Feeder::resolve_duplicate`].
    async fn resolve_once(&self, merge: &Merge, context: &Context) -> Result<Applied, FeedError> {
        let keys = self.store.sealer();
        let patient = merge.patient();
        let mut transaction = self.store.pool().begin().await.map_err(store)?;
        let domains = record::domains(&mut *transaction).await?;
        let (domain, subsumed) = condition(&domains, merge.subsumed())?;
        let (_, survivor) = condition(&domains, merge.survivor())?;
        let carried = write::carried(patient, &domains, keys)?;
        let subsumed_hmac = keys.hmac(domain.id.uuid(), &subsumed);
        let survivor_hmac = keys.hmac(domain.id.uuid(), &survivor);
        let mut identities = identity_keys(&subsumed_hmac, &carried);
        identities.push(write::identity_key(&survivor_hmac));
        write::advisory(&mut transaction, identities).await?;
        let Some(kept) = write::lock(&mut transaction, domain.id, &survivor_hmac).await? else {
            return Err(FeedError::SurvivorNotFound);
        };
        if !kept.active || kept.removed {
            return Err(FeedError::SurvivorNotFound);
        }
        let locked = write::lock_or_insert(&mut transaction, keys, domain, &subsumed).await?;
        self.write_version(&mut transaction, &locked, patient, &carried, context)
            .await?;
        let action = write::action(
            &mut transaction,
            "resolve-duplicate",
            context,
            Subjects {
                record: Some(locked.id),
                ..Subjects::default()
            },
        )
        .await?;
        sqlx::query("UPDATE pix.source_record SET active = false, replaced_by = $2 WHERE id = $1")
            .bind(locked.id.uuid())
            .bind(kept.id.uuid())
            .execute(&mut *transaction)
            .await
            .map_err(store)?;
        let former = link::close_link(&mut transaction, locked.id, action).await?;
        let (subject, survivor_carried, version) =
            current(&mut transaction, keys, kept.id, &domains).await?;
        write::advisory(
            &mut transaction,
            survivor_carried
                .iter()
                .filter(|c| c.kind == DomainKind::Authoritative)
                .map(|c| write::identity_key(&c.hmac))
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
                &mut transaction,
                vec![write::person_key(former), write::person_key(held)],
            )
            .await?;
            link::merge(&mut transaction, former, held, MergeKind::Source, context).await?;
            merged.push(former);
        }
        let me = me(kept.id, version, &subject, &survivor_carried);
        let linked = link::cross_reference(&mut transaction, self, &me, context).await?;
        transaction.commit().await.map_err(store)?;
        merged.extend(linked.merged);
        tracing::info!(
            record = %locked.id.uuid(),
            survivor = %kept.id.uuid(),
            merged = merged.len(),
            "a duplicate is resolved"
        );
        Ok(Applied {
            action: Action::ResolveDuplicate,
            record: locked.id,
            person: linked.person,
            merged,
            candidates: linked.candidates,
            scored: linked.scored,
        })
    }

    /// Writes the next version of `locked` from `patient` with the
    /// identifiers it carries, and returns its number and its subject.
    async fn write_version(
        &self,
        connection: &mut PgConnection,
        locked: &write::Locked,
        patient: &Patient,
        carried: &[Carried],
        context: &Context,
    ) -> Result<(i32, Subject), FeedError> {
        let stored = demographics::stored(patient)?;
        let subject = Subject::from_patient(patient, local_keys(carried));
        let blocks = Blocks {
            phonetic: blocking::text_key(BlockingKey::BirthYearFamilyPhonetic, &subject),
            postal: blocking::text_key(BlockingKey::PostalCodeBirthDate, &subject),
        };
        let version = write::add_version(connection, locked, &stored, &blocks, context).await?;
        write::insert_carried(connection, self.store.sealer(), locked.id, version, carried).await?;
        Ok((version, subject))
    }
}

/// Returns the domain and the normalised value of `identifier`.
fn condition<'d>(
    domains: &'d [Domain],
    identifier: &SourceIdentifier,
) -> Result<(&'d Domain, crate::store::seal::IdentifierValue), FeedError> {
    let system = identifier.system();
    let domain = domains
        .iter()
        .find(|d| d.system == system && !d.retired)
        .ok_or(FeedError::UnknownDomain)?;
    let value = Rule::of(system)
        .normalise(identifier.value().expose_secret())
        .map_err(|source| FeedError::Identifier {
            system: system.to_owned(),
            source,
        })?;
    Ok((domain, value))
}

/// Returns the advisory keys of the record's own identifier and of every
/// authoritative identifier it carries.
fn identity_keys(own: &[u8; 32], carried: &[Carried]) -> Vec<i64> {
    std::iter::once(write::identity_key(own))
        .chain(
            carried
                .iter()
                .filter(|c| c.kind == DomainKind::Authoritative)
                .map(|c| write::identity_key(&c.hmac)),
        )
        .collect()
}

/// Returns the keys of the identifiers `carried` holds in `local` domains.
fn local_keys(carried: &[Carried]) -> BTreeSet<IdentifierKey> {
    carried
        .iter()
        .filter(|c| c.kind == DomainKind::Local)
        .map(|c| IdentifierKey::new(c.domain.uuid(), c.hmac))
        .collect()
}

/// Returns the version the cross-referencing runs over.
fn me<'s>(record: RecordId, version: i32, subject: &'s Subject, carried: &[Carried]) -> Me<'s> {
    let keys = |kind: DomainKind| {
        carried
            .iter()
            .filter(|c| c.kind == kind)
            .map(|c| IdentifierKey::new(c.domain.uuid(), c.hmac))
            .collect()
    };
    Me {
        record,
        version,
        subject,
        authoritative: keys(DomainKind::Authoritative),
        local: keys(DomainKind::Local),
        phonetic: blocking::text_key(BlockingKey::BirthYearFamilyPhonetic, subject),
        postal: blocking::text_key(BlockingKey::PostalCodeBirthDate, subject),
    }
}

/// A carried identifier as stored: domain, HMAC, sealed value, plain value.
type CarriedRow = (Uuid, Vec<u8>, Option<Vec<u8>>, Option<String>);

/// Reads the current version of `record`: its subject, the identifiers it
/// carries, opened, and its number.
async fn current(
    connection: &mut PgConnection,
    keys: &crate::store::seal::Sealer,
    record: RecordId,
    domains: &[Domain],
) -> Result<(Subject, Vec<Carried>, i32), FeedError> {
    let (version, text): (i32, String) = sqlx::query_as(
        "SELECT v.version, v.demographics::text
         FROM pix.source_record r
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
         WHERE r.id = $1",
    )
    .bind(record.uuid())
    .fetch_one(&mut *connection)
    .await
    .map_err(store)?;
    let rows: Vec<CarriedRow> = sqlx::query_as(
        "SELECT domain_id, identifier_hmac, identifier_sealed, identifier_value
         FROM pix.carried_identifier WHERE record_id = $1 AND version = $2
         ORDER BY domain_id, identifier_hmac",
    )
    .bind(record.uuid())
    .bind(version)
    .fetch_all(&mut *connection)
    .await
    .map_err(store)?;
    let malformed = || FeedError::Store(StoreError::Seal(crate::store::seal::SealError::Malformed));
    let mut carried = Vec::with_capacity(rows.len());
    for (domain_id, hmac, sealed, plain) in rows {
        let Some(domain) = domains.iter().find(|d| d.id.uuid() == domain_id) else {
            return Err(malformed());
        };
        let value = match (sealed, plain) {
            (Some(sealed), _) => keys.open(domain_id, &sealed).map_err(StoreError::Seal)?,
            (None, Some(plain)) => {
                crate::store::seal::IdentifierValue::new(plain).ok_or_else(malformed)?
            }
            (None, None) => return Err(malformed()),
        };
        carried.push(Carried {
            domain: domain.id,
            kind: domain.kind,
            sensitive: domain.sensitive,
            value,
            hmac: hmac.try_into().map_err(|_short| malformed())?,
        });
    }
    let patient = demographics::patient(&text)?;
    Ok((
        Subject::from_patient(&patient, local_keys(&carried)),
        carried,
        version,
    ))
}

/// Writes the BALP record of one ITI-104 message, after the transaction.
///
/// The record is the PIXm Feed Create, Update or Delete Manager audit
/// profile (PIXm 3.1.0 §2:3.104.5.1.2) of `identifier`, the condition, with
/// the person as the data and `outcome` as its outcome, in the request's
/// place in the spool.
///
/// A message refused before it applied is recorded as an update or a
/// delete that failed.
///
/// # Errors
/// Returns [`Unrecorded`] when the record cannot be written or stored.
pub async fn record(
    audit: &Audit,
    pending: Option<&Pending>,
    call: &Call,
    identifier: &SourceIdentifier,
    removal: bool,
    outcome: Result<&Applied, &FeedError>,
) -> Result<(), Unrecorded> {
    let identifier = Identifier {
        system: identifier.system().to_owned(),
        value: identifier.value().clone(),
    };
    let (action, person, outcome) = match outcome {
        Ok(applied) => (
            applied.action.audited(),
            applied.person.map(|p| format!("Patient/{}", p.uuid())),
            Outcome::Success,
        ),
        Err(_) if removal => (records::Feed::Delete, None, Outcome::MinorFailure),
        Err(_) => (records::Feed::Update, None, Outcome::MinorFailure),
    };
    let exchange = records::feed(call, action, &identifier, person.as_deref(), outcome);
    match pending {
        Some(pending) => pending.record(audit, &exchange).await,
        None => audit.record(vec![exchange]).await,
    }
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
    /// Concurrent transactions moved what this one read.
    #[error("concurrent transactions moved the persons this feed joins")]
    Contention,
    /// A statement on the store failed.
    #[error("the store refused the feed")]
    Store(#[from] StoreError),
}

impl FeedError {
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
