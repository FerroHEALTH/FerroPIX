// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A steward's write of an identifier or a pseudonymisation domain.
//!
//! The row is inserted, or locked and changed, in one transaction, and the
//! steward action and its BALP record are written in that transaction
//! before it commits.
//!
//! No specification governs this: our own design.

use axum::extract::Request;
use axum::response::Response;
use ihe_atna::balp::Exchange;
use serde::de::DeserializeOwned;
use sqlx::postgres::PgArguments;
use sqlx::query::QueryScalar;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::audit::records::Call;
use crate::operator::{Operator, OperatorError, Recorded, record_action, wire};
use crate::problem::{self, Kind};

/// One kind of domain a steward writes: the statements of a change, each
/// with its table and columns written out, and the details of its refusals.
#[derive(Debug, Clone, Copy)]
pub struct Domains {
    /// Selects the row `$1` for update: its key and whether it is retired.
    pub lock: &'static str,
    /// Sets the row `$1`'s name to `$2` and the client that holds it to `$3`
    /// where they are given, and retires it when `$4` holds.
    pub update: &'static str,
    /// The refusal of a key another domain holds.
    pub taken: &'static str,
    /// The refusal of an id that names no domain.
    pub missing: &'static str,
    /// The refusal of a change of a retired domain.
    pub retired: &'static str,
    /// The kinds of the steward action that creates a domain and that
    /// changes one.
    pub actions: (&'static str, &'static str),
}

/// The open transaction and the write of an insert or a change, or its
/// refusal.
pub type Written<'a> = Result<(Transaction<'static, Postgres>, Write<'a>), Box<Response>>;

/// One domain write, its statement run.
#[derive(Debug, Clone)]
pub struct Write<'a> {
    /// Whether the domain was created.
    pub created: bool,
    /// The domain's key: the system of an identifier domain, the namespace
    /// of a pseudonymisation domain.
    pub key: String,
    /// The domain's id.
    pub id: Uuid,
    /// The reason given.
    pub reason: Option<&'a str>,
}

/// What a change asks of a domain row.
#[derive(Debug, Clone, Copy)]
pub struct Asked<'a> {
    /// The new name.
    pub name: Option<&'a str>,
    /// The new client that holds the domain: the Source of an identifier
    /// domain, the consumer of a pseudonymisation domain.
    pub holder: Option<&'a str>,
    /// Whether the domain is retired.
    pub retire: bool,
    /// The reason given.
    pub reason: &'a str,
}

/// What the statement of a change found.
enum Changed {
    /// No domain has the id.
    Missing,
    /// The domain is retired.
    Retired,
    /// The domain of this key was changed in this open transaction.
    Written(Transaction<'static, Postgres>, String),
}

/// The parts, the domain id of the path and the change `T` a `PATCH`
/// posts.
///
/// # Errors
///
/// The refusal of a path that names no domain, `missing`, and of a body that
/// is no `T`.
pub async fn requested<T: DeserializeOwned>(
    request: Request,
    missing: &'static str,
) -> Result<(http::request::Parts, Uuid, T), Box<Response>> {
    let (parts, body) = request.into_parts();
    let Some(id) = wire::path_id(&parts) else {
        return Err(Box::new(problem::refuse(Kind::NotFound, missing)));
    };
    let (change, _bytes) = wire::posted::<T>(&parts, body).await?;
    Ok((parts, id, change))
}

/// Whether a field a write gives is there and blank.
#[must_use]
pub fn blank(text: Option<&str>) -> bool {
    text.is_some_and(|text| text.trim().is_empty())
}

/// Runs `insert`, which returns the id of the domain of `key` it wrote or
/// none when the key is held, in a new transaction, and returns the open
/// transaction and the write given `reason`.
///
/// # Errors
///
/// The refusal of a held key and the answer of a store that fails.
pub async fn insert<'a>(
    operator: &Operator,
    domains: &Domains,
    insert: QueryScalar<'_, Postgres, Uuid, PgArguments>,
    (key, reason): (&str, Option<&'a str>),
) -> Result<(Transaction<'static, Postgres>, Write<'a>), Box<Response>> {
    let outcome = async {
        let mut transaction = operator.store.pool().begin().await?;
        let id = insert.fetch_optional(&mut *transaction).await?;
        Ok::<_, OperatorError>(id.map(|id| (transaction, id)))
    }
    .await;
    match outcome {
        Ok(Some((transaction, id))) => Ok((
            transaction,
            Write {
                created: true,
                key: key.to_owned(),
                id,
                reason,
            },
        )),
        Ok(None) => Err(Box::new(problem::refuse(Kind::AlreadyHeld, domains.taken))),
        Err(error) => Err(Box::new(wire::failed(&error))),
    }
}

/// Locks the domain `id` and changes it as `asked`, and returns the open
/// transaction and the write.
///
/// # Errors
///
/// The refusal of an id that names no domain or a retired one, and the
/// answer of a store that fails.
pub async fn change<'a>(
    operator: &Operator,
    domains: &Domains,
    (id, asked): (Uuid, Asked<'a>),
) -> Result<(Transaction<'static, Postgres>, Write<'a>), Box<Response>> {
    let outcome = async {
        let mut transaction = operator.store.pool().begin().await?;
        let current: Option<(String, bool)> = sqlx::query_as(domains.lock)
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await?;
        let Some((key, retired)) = current else {
            return Ok(Changed::Missing);
        };
        if retired {
            return Ok(Changed::Retired);
        }
        sqlx::query(domains.update)
            .bind(id)
            .bind(asked.name.map(str::trim))
            .bind(asked.holder.map(str::trim))
            .bind(asked.retire)
            .execute(&mut *transaction)
            .await?;
        Ok::<_, OperatorError>(Changed::Written(transaction, key))
    }
    .await;
    match outcome {
        Ok(Changed::Written(transaction, key)) => Ok((
            transaction,
            Write {
                created: false,
                key,
                id,
                reason: Some(asked.reason),
            },
        )),
        Ok(Changed::Missing) => Err(Box::new(problem::refuse(Kind::NotFound, domains.missing))),
        Ok(Changed::Retired) => Err(Box::new(problem::refuse(Kind::Retired, domains.retired))),
        Err(error) => Err(Box::new(wire::failed(&error))),
    }
}

/// Writes the steward action of `written`, which `on` names the domain in,
/// and the BALP record `exchange` makes of it, in its open transaction, and
/// commits it; returns the write.
///
/// # Errors
///
/// The refusal `written` carries, the answer of a store that fails and of an
/// audit trail that does not take the record.
pub async fn commit<'a>(
    (operator, parts): (&Operator, &http::request::Parts),
    domains: &Domains,
    written: Written<'a>,
    on: impl FnOnce(&mut Recorded, &Write<'_>),
    exchange: impl FnOnce(&Call, &Write<'_>) -> Exchange,
) -> Result<Write<'a>, Box<Response>> {
    let (mut transaction, write) = written?;
    let failed = |error: OperatorError| Box::new(wire::failed(&error));
    let mut recorded = Recorded {
        kind: if write.created {
            domains.actions.0
        } else {
            domains.actions.1
        },
        actor: wire::actor(parts),
        reason: write
            .reason
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(str::to_owned),
        request_id: wire::request_id(parts),
        ..Recorded::default()
    };
    on(&mut recorded, &write);
    record_action(&mut *transaction, &recorded)
        .await
        .map_err(failed)?;
    let exchange = exchange(&wire::call(operator, parts), &write);
    wire::record(operator, parts, vec![exchange])
        .await
        .map_err(|error| Box::new(wire::unaudited(&error)))?;
    transaction
        .commit()
        .await
        .map_err(|error| failed(error.into()))?;
    Ok(write)
}
