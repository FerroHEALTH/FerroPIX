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

use fhir_types::r4::patient::Patient;
use sqlx::PgConnection;

use crate::feed::{Applied, Context, FeedError, Feeder, Resolving, Revising};
use crate::store::record::PersonId;

/// The master identity an ITI-93 create or update writes.
#[derive(Debug, Clone, Copy)]
pub(super) struct Identity {
    /// The person the record names by its value.
    pub(super) identity: PersonId,
    /// Whether the record must be held already: an update.
    pub(super) known: bool,
}

impl Feeder {
    /// Applies an ITI-93 create of the master identity `id` in the master
    /// domain `system`, with `patient` as its demographics.
    ///
    /// # Errors
    /// Returns [`FeedError::Subsumed`] when the identity was merged, and
    /// the errors of [`Feeder::revise`] otherwise; nothing is committed.
    pub async fn create_master(
        &self,
        system: &str,
        id: PersonId,
        patient: &Patient,
        context: &Context,
    ) -> Result<Applied, FeedError> {
        self.master(system, id, false, patient, context).await
    }

    /// Applies an ITI-93 update of the master identity `id` in the master
    /// domain `system`, with `patient` as its demographics.
    ///
    /// # Errors
    /// Returns [`FeedError::NotFound`] when the store holds no such master
    /// record or removed it, [`FeedError::Subsumed`] when the identity was
    /// merged, and the errors of [`Feeder::revise`] otherwise; nothing is
    /// committed.
    pub async fn update_master(
        &self,
        system: &str,
        id: PersonId,
        patient: &Patient,
        context: &Context,
    ) -> Result<Applied, FeedError> {
        self.master(system, id, true, patient, context).await
    }

    /// Applies an ITI-93 merge: the master identity `deprecated` is replaced
    /// by `survivor` in the master domain `system`, `patient` the deprecated
    /// Patient (§2:3.93.4.1.2.4).
    ///
    /// # Errors
    /// As [`Feeder::resolve_duplicate`]; nothing is committed.
    pub async fn merge_master(
        &self,
        system: &str,
        deprecated: PersonId,
        survivor: PersonId,
        patient: &Patient,
        context: &Context,
    ) -> Result<Applied, FeedError> {
        let subsumed = deprecated.uuid().to_string();
        let kept = survivor.uuid().to_string();
        let resolving = Resolving {
            system,
            subsumed: &subsumed,
            survivor: &kept,
        };
        self.resolving(&resolving, patient, context).await
    }

    /// Applies an ITI-93 delete: the master record of `id` in the master
    /// domain `system` is removed.
    ///
    /// # Errors
    /// As [`Feeder::remove`]; nothing is committed.
    pub async fn delete_master(
        &self,
        system: &str,
        id: PersonId,
        context: &Context,
    ) -> Result<Applied, FeedError> {
        self.removing(system, &id.uuid().to_string(), context).await
    }

    /// Writes the master identity `id`, held already when `known`.
    async fn master(
        &self,
        system: &str,
        id: PersonId,
        known: bool,
        patient: &Patient,
        context: &Context,
    ) -> Result<Applied, FeedError> {
        let value = id.uuid().to_string();
        let revising = Revising {
            system,
            value: &value,
            master: Some(Identity {
                identity: id,
                known,
            }),
        };
        self.revising(&revising, patient, context).await
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
