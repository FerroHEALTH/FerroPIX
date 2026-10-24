// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The cross-reference store: schema `pix` in PostgreSQL 18.
//!
//! [`schema`] holds the embedded migration set, [`gate`] the boot gate that
//! refuses a role able to read a schema beside `pix`, [`seal`] the sealing of
//! identifier values, [`key`] the check that the configured key is the one the
//! store was written under, [`seed`] the `[[domain]]` seed, [`record`] the
//! source records found by their identifier, [`identifier`] the rule each
//! domain's values are normalised by, [`xref`] the cross-reference a PIX
//! query reads, [`estimate`] the field frequencies `matching estimate`
//! reads, [`reseal`] the key rotation and [`indicator`] the readiness
//! indicator.
//! [`Store::start`] runs them in the order a job needs.
//!
//! PIXm fixes what the store must answer and what the feed does to it (PIXm
//! 3.1.0 §2:3.83.4.1.3, §2:3.104.4.1.3, §2:3.104.4.2.3, §2:3.104.4.3.3); no
//! specification governs how it is stored: our own design
//! (`docs/architecture.md` §4).

pub mod estimate;
pub mod gate;
pub mod identifier;
pub mod indicator;
pub mod key;
pub mod record;
pub mod reseal;
pub mod schema;
pub mod seal;
pub mod seed;
pub mod xref;

use std::error::Error;
use std::fmt;
use std::str::FromStr as _;

use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::config::section::db::DbSettings;
use crate::config::section::domain::DomainSeed;
use crate::store::seal::{KeyId, SealError, Sealer};

/// The `application_name` every connection reports, so an operator finds the
/// server's sessions in `pg_stat_activity`.
const APPLICATION_NAME: &str = "ferropix";

/// A pool of connections to the store, with the keys of its sealing.
#[derive(Debug, Clone)]
pub struct Store {
    /// The connection pool.
    pool: PgPool,
    /// The keys the configured sealing key yields.
    sealer: std::sync::Arc<Sealer>,
}

/// The job a store is started for, which decides the steps it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    /// `serve`: the boot gate, then the migrations unless `[db] migrate =
    /// false`, the key check and the seed.
    Serve,
    /// `db migrate`: the migrations, the key check and the seed, as the role
    /// the migrations run under, which the boot gate does not hold to `pix`.
    Migrate,
}

impl Store {
    /// Connects to the store `db` names and runs the steps `job` needs.
    ///
    /// # Errors
    /// Returns [`StoreError::Url`] for a URL the driver refuses,
    /// [`StoreError::Connect`] when no connection opens, every refusal of
    /// [`gate::check`], [`schema::apply`], [`key::verify`] and
    /// [`seed::apply`], and [`StoreError::Seal`] when the keys cannot be
    /// derived.
    pub async fn start(
        db: &DbSettings,
        domains: &[DomainSeed],
        job: Job,
    ) -> Result<Self, StoreError> {
        let store = Self::connect(db).await?;
        if job == Job::Serve {
            gate::check(&store).await?;
        }
        if job == Job::Migrate || db.migrate {
            schema::apply(&store).await?;
        }
        key::verify(&store).await?;
        seed::apply(&store, domains).await?;
        Ok(store)
    }

    /// Connects to the store `db` names, and runs nothing.
    ///
    /// # Errors
    /// Returns [`StoreError::Url`] for a URL the driver refuses,
    /// [`StoreError::Connect`] when no connection opens and
    /// [`StoreError::Seal`] when the keys cannot be derived.
    pub async fn connect(db: &DbSettings) -> Result<Self, StoreError> {
        let options = PgConnectOptions::from_str(db.url.expose())
            .map_err(|source| StoreError::Url(DbFailure(source)))?
            .application_name(APPLICATION_NAME);
        let pool = PgPoolOptions::new()
            .max_connections(db.pool_size)
            .connect_with(options)
            .await
            .map_err(|source| StoreError::Connect(DbFailure(source)))?;
        let sealer = Sealer::new(&db.data_key)?;
        Ok(Self {
            pool,
            sealer: std::sync::Arc::new(sealer),
        })
    }

    /// Returns a store over `db` whose connections open on first use, and
    /// runs nothing: a statement fails as a refused connection does.
    ///
    /// It serves a router whose routes may never reach the database, such as
    /// the guard's tests; `serve` uses [`Store::start`].
    ///
    /// # Errors
    /// Returns [`StoreError::Url`] for a URL the driver refuses and
    /// [`StoreError::Seal`] when the keys cannot be derived.
    pub fn lazy(db: &DbSettings) -> Result<Self, StoreError> {
        let options = PgConnectOptions::from_str(db.url.expose())
            .map_err(|source| StoreError::Url(DbFailure(source)))?
            .application_name(APPLICATION_NAME);
        let pool = PgPoolOptions::new()
            .max_connections(db.pool_size)
            .connect_lazy_with(options);
        let sealer = Sealer::new(&db.data_key)?;
        Ok(Self {
            pool,
            sealer: std::sync::Arc::new(sealer),
        })
    }

    /// Returns the connection pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Returns the keys of the configured sealing key.
    #[must_use]
    pub fn sealer(&self) -> &Sealer {
        &self.sealer
    }

    /// Closes every connection of the pool.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// A store that cannot be started or a statement on it that failed.
///
/// No variant carries an identifier value or a demographic: a database
/// refusal keeps its SQLSTATE, its constraint and its message, and drops the
/// detail PostgreSQL fills with the row.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The driver refuses `db.url`.
    #[error("db.url is refused by the PostgreSQL driver")]
    Url(#[source] DbFailure),
    /// No connection to the store opens.
    #[error("cannot connect to the store")]
    Connect(#[source] DbFailure),
    /// The migration set cannot be applied.
    #[error("the migrations cannot be applied")]
    Migrate(#[source] sqlx::migrate::MigrateError),
    /// A statement on the store failed.
    #[error("a statement on the store failed")]
    Query(#[source] DbFailure),
    /// The connected role is a superuser, which reads every schema.
    #[error(
        "the role {role} is a superuser, which reads every schema; connect as a role that owns pix alone"
    )]
    Superuser {
        /// The connected role.
        role: String,
    },
    /// The connected role holds `pg_read_all_data`, which reads every schema.
    #[error("the role {role} holds pg_read_all_data, which reads every schema")]
    ReadsAllData {
        /// The connected role.
        role: String,
    },
    /// The connected role can read a relation outside schema `pix`.
    #[error("the role {role} can read the {kind} {relation} outside schema pix")]
    ReadsOtherSchema {
        /// The connected role.
        role: String,
        /// What the relation is: a table, a view, a sequence.
        kind: String,
        /// The schema-qualified relation.
        relation: String,
    },
    /// The store's identifiers are written under a key other than the
    /// configured one.
    #[error(
        "the store's identifiers are written under key {stored}, and db.data_key_file holds key {configured}"
    )]
    KeyMismatch {
        /// The key the store names.
        stored: KeyId,
        /// The key the configuration holds.
        configured: KeyId,
    },
    /// A `db reseal` to another key has not finished.
    #[error("a db reseal from key {stored} to key {pending} has not finished; run it again")]
    ResealPending {
        /// The key the rows not yet resealed are written under.
        stored: KeyId,
        /// The key the reseal moves them to.
        pending: KeyId,
    },
    /// A stored domain names a kind the schema does not admit.
    #[error("the stored domain {system} names a kind the schema does not admit")]
    UnknownDomainKind {
        /// The domain's system.
        system: String,
    },
    /// `db reseal` was given an old key the store is not written under.
    #[error("the store is written under key {stored}, and the old key is key {old}")]
    OldKeyUnknown {
        /// The key the store names.
        stored: KeyId,
        /// The old key `db reseal` was given.
        old: KeyId,
    },
    /// `db reseal` was given the configured key as the old one.
    #[error("the old key is the configured key; there is nothing to reseal")]
    SameKey,
    /// A sealed identifier cannot be sealed or opened.
    #[error("an identifier cannot be sealed or opened")]
    Seal(#[from] SealError),
    /// A record holds the most versions a version number counts.
    #[error("the record holds the most versions a version number counts")]
    VersionOverflow,
}

impl StoreError {
    /// Returns the database failure under this error, when it is one.
    #[must_use]
    pub fn database(&self) -> Option<&DbFailure> {
        match self {
            Self::Url(failure) | Self::Connect(failure) | Self::Query(failure) => Some(failure),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for StoreError {
    fn from(source: sqlx::Error) -> Self {
        Self::Query(DbFailure(source))
    }
}

/// A failure the PostgreSQL driver reported, rendered without the data it
/// may carry.
///
/// PostgreSQL puts the values of the offending row in an error's detail
/// (`Key (…)=(…)`, `Failing row contains (…)`), which the driver's own
/// `Debug` shows. This type renders the SQLSTATE, the constraint, the table
/// and the message, and never the detail, through `Display` and `Debug`
/// alike, and stops the source chain at a database error for the same
/// reason. No specification governs this: our own design.
pub struct DbFailure(sqlx::Error);

impl DbFailure {
    /// Returns the SQLSTATE of a database refusal.
    #[must_use]
    pub fn code(&self) -> Option<String> {
        self.0
            .as_database_error()
            .and_then(|error| error.code().map(std::borrow::Cow::into_owned))
    }

    /// Returns the constraint a database refusal names.
    #[must_use]
    pub fn constraint(&self) -> Option<&str> {
        self.0
            .as_database_error()
            .and_then(|error| error.constraint())
    }
}

impl fmt::Display for DbFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(error) = self.0.as_database_error() else {
            return fmt::Display::fmt(&self.0, f);
        };
        write!(f, "the database refused the statement")?;
        if let Some(code) = error.code() {
            write!(f, " (SQLSTATE {code})")?;
        }
        if let Some(constraint) = error.constraint() {
            write!(f, " on the constraint {constraint}")?;
        } else if let Some(table) = error.table() {
            write!(f, " on the table {table}")?;
        }
        write!(f, ": {}", error.message())
    }
}

impl fmt::Debug for DbFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DbFailure({self})")
    }
}

impl Error for DbFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if self.0.as_database_error().is_some() {
            return None;
        }
        Some(&self.0)
    }
}
