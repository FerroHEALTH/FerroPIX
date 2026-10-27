// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The readiness indicator of the store: down while no statement reaches
//! PostgreSQL.
//!
//! No specification governs health probes: our own design
//! (`docs/architecture.md` §9).

use ferrohealth_health::registry::{Check, HealthIndicator, Outcome};

use crate::store::{DbFailure, Store};

/// The name the indicator answers under in the readiness document.
pub const NAME: &str = "database";

/// Asks the store one trivial statement.
#[derive(Debug, Clone)]
pub struct Database {
    /// The store asked.
    store: Store,
}

impl Database {
    /// Returns the indicator of `store`.
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

impl HealthIndicator for Database {
    fn name(&self) -> &'static str {
        NAME
    }

    fn check(&self) -> Check<'_> {
        Box::pin(async move {
            match sqlx::query("SELECT 1").execute(self.store.pool()).await {
                Ok(_) => Outcome::up(),
                Err(error) => {
                    Outcome::down(format!("the store does not answer: {}", DbFailure(error)))
                }
            }
        })
    }
}
