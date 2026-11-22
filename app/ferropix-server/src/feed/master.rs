// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The master identities an ITI-93 feed message changes, as the Patient
//! Identity Registry persists them (PMIR 1.6.0 §2:3.93.4.1.3).
//!
//! A master identity is a person whose id the Source assigned, with a
//! record in the master domain whose value is that id: PMIR has the
//! Registry assign `Patient.id` (§2:3.93.4.1.1) and gives its response no
//! place to return one (§2:3.93.4.2.2), so the Source's id is kept. A create
//! makes the person with that id when the store lacks it and links the
//! record to it; a create of a held identity revises it. An update revises
//! the held record and refuses one the store lacks or removed. A create or
//! an update of an identity that was merged would undo the merge, which
//! [`FeedError::Subsumed`] refuses. A merge is the Source's own duplicate
//! resolution in the master domain, and a delete removes the master record.
//! Every change of one message runs in one transaction with the message's
//! BALP record ([`Feeder::message`]): PMIR 1.6.0 §2:3.93.4.1.3 asks for a
//! correlated outcome per entry and is silent on whether the entries commit
//! together, so committing them together with their record is our own
//! design (#742).

use fhir_types::r4::patient::Patient;
use ihe_atna::balp::Exchange;
use sqlx::{Acquire as _, PgConnection};

use crate::audit::Audit;
use crate::feed::{
    ATTEMPTS, Applied, Context, FeedError, Feeder, Resolving, Revising, Step, store,
};
use crate::store::record::PersonId;

/// The master identity an ITI-93 create or update writes.
#[derive(Debug, Clone, Copy)]
pub(super) struct Identity {
    /// The person the record names by its value.
    pub(super) identity: PersonId,
    /// Whether the record must be held already: an update.
    pub(super) known: bool,
}

/// Whether an ITI-93 message creates a master identity or updates one held
/// already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MasterChange {
    /// A create.
    Create,
    /// An update of a held identity.
    Update,
}

/// One change of an ITI-93 message the store writes.
#[derive(Debug, Clone, Copy)]
pub enum MasterStep<'a> {
    /// A create or an update of the master identity `id`, with `patient`
    /// as its demographics.
    Write {
        /// A create or an update.
        change: MasterChange,
        /// The master identity.
        id: PersonId,
        /// Its demographics.
        patient: &'a Patient,
    },
    /// The master identity `deprecated` replaced by `survivor`, `patient`
    /// the deprecated Patient (§2:3.93.4.1.2.4).
    Merge {
        /// The deprecated identity.
        deprecated: PersonId,
        /// The surviving identity.
        survivor: PersonId,
        /// The deprecated Patient.
        patient: &'a Patient,
    },
    /// The master record of `id` removed.
    Delete {
        /// The master identity.
        id: PersonId,
    },
}

/// What became of each change of one message: `None` for a change that
/// needed no write, the change's outcome otherwise.
pub type Outcomes = Vec<Option<Result<Applied, FeedError>>>;

/// The records of one message, written from the outcomes of its changes.
pub type MessageRecords<'a> = &'a (dyn Fn(&Outcomes) -> Vec<Exchange> + Sync);

impl Feeder {
    /// Applies the changes of one ITI-93 message to the master domain
    /// `system` in one transaction, which holds the message's records too,
    /// written from the outcomes by `records`, and tries the whole message
    /// again after contention.
    ///
    /// A change refused on its own ([`FeedError::refuses_change`]) is rolled
    /// back to its savepoint and the others go on; any other failure rolls
    /// back the whole message, so its changes commit together with their
    /// record or not at all (#742).
    ///
    /// # Errors
    /// Returns the [`FeedError`] that ended the transaction, a
    /// [`FeedError::Contention`] on every attempt included, and
    /// [`FeedError::Unrecorded`] when the records cannot be kept; nothing
    /// is committed.
    pub async fn message(
        &self,
        system: &str,
        changes: &[Option<MasterStep<'_>>],
        context: &Context,
        audit: &Audit,
        records: MessageRecords<'_>,
    ) -> Result<Outcomes, FeedError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self
                .message_once(system, changes, context, audit, records)
                .await
            {
                Err(error) if error.retryable() && attempt < ATTEMPTS => {
                    tracing::debug!(attempt, "an ITI-93 message is tried again after contention");
                }
                Ok(outcomes) => {
                    self.store.announced();
                    audit.committed();
                    return Ok(outcomes);
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// One attempt at [`Feeder::message`].
    async fn message_once(
        &self,
        system: &str,
        changes: &[Option<MasterStep<'_>>],
        context: &Context,
        audit: &Audit,
        records: MessageRecords<'_>,
    ) -> Result<Outcomes, FeedError> {
        let mut transaction = self.store.pool().begin().await.map_err(store)?;
        let mut outcomes = Vec::with_capacity(changes.len());
        for change in changes {
            let Some(change) = change else {
                outcomes.push(None);
                continue;
            };
            let deprecated;
            let kept;
            let value;
            let step = match *change {
                MasterStep::Write {
                    change,
                    id,
                    patient,
                } => {
                    value = id.uuid().to_string();
                    Step::Revise(
                        Revising {
                            system,
                            value: &value,
                            master: Some(Identity {
                                identity: id,
                                known: matches!(change, MasterChange::Update),
                            }),
                        },
                        patient,
                    )
                }
                MasterStep::Merge {
                    deprecated: from,
                    survivor,
                    patient,
                } => {
                    deprecated = from.uuid().to_string();
                    kept = survivor.uuid().to_string();
                    Step::Resolve(
                        Resolving {
                            system,
                            subsumed: &deprecated,
                            survivor: &kept,
                        },
                        patient,
                    )
                }
                MasterStep::Delete { id } => {
                    value = id.uuid().to_string();
                    Step::Remove {
                        system,
                        value: &value,
                        deleted: true,
                    }
                }
            };
            let mut savepoint = transaction.begin().await.map_err(store)?;
            match self.step_in(&mut savepoint, step, context).await {
                Ok(applied) => {
                    savepoint.commit().await.map_err(store)?;
                    outcomes.push(Some(Ok(applied)));
                }
                Err(error) if error.refuses_change() => {
                    savepoint.rollback().await.map_err(store)?;
                    outcomes.push(Some(Err(error)));
                }
                Err(error) => return Err(error),
            }
        }
        let exchanges = records(&outcomes);
        audit
            .enlist(&mut transaction, &exchanges)
            .await
            .map_err(FeedError::Unrecorded)?;
        transaction.commit().await.map_err(store)?;
        Ok(outcomes)
    }
}

/// Returns whether the person `id` was merged into another.
pub(super) async fn merged(connection: &mut PgConnection, id: PersonId) -> Result<bool, FeedError> {
    let merged: Option<bool> =
        sqlx::query_scalar("SELECT status = 'merged' FROM pix.person WHERE id = $1")
            .bind(id.uuid())
            .fetch_optional(&mut *connection)
            .await
            .map_err(|source| FeedError::Store(source.into()))?;
    Ok(merged.unwrap_or(false))
}

/// Inserts the person `id` when the store lacks it, the id a Source assigned
/// a master identity, and returns it.
pub(super) async fn person_with_id(
    connection: &mut PgConnection,
    id: PersonId,
) -> Result<PersonId, FeedError> {
    sqlx::query("INSERT INTO pix.person (id) VALUES ($1) ON CONFLICT (id) DO NOTHING")
        .bind(id.uuid())
        .execute(&mut *connection)
        .await
        .map_err(|source| FeedError::Store(source.into()))?;
    Ok(id)
}
