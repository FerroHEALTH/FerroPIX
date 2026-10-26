// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[outbox]` section: the retention of a settled message.
//!
//! It sets how long a delivered or abandoned message is kept before the
//! sender prunes it, and how many it deletes at a time. A pending message is
//! never pruned. No specification governs retention: our own design.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::error::Error;

/// The `[outbox]` section as written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Outbox {
    /// The days a delivered or abandoned message is kept.
    pub retention_days: u32,
    /// The most messages one prune deletes in one statement.
    pub prune_batch: u32,
}

impl Default for Outbox {
    fn default() -> Self {
        Self {
            retention_days: 30,
            prune_batch: 500,
        }
    }
}

/// The `[outbox]` section, resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    /// How long a delivered or abandoned message is kept.
    pub keep: Duration,
    /// The most messages one statement deletes.
    pub batch: u32,
}

impl Outbox {
    /// Resolves the section.
    ///
    /// # Errors
    /// Returns [`Error::Value`] for a zero retention or batch.
    pub fn resolve(&self) -> Result<Retention, Error> {
        for (key, zero) in [
            ("outbox.retention_days", self.retention_days == 0),
            ("outbox.prune_batch", self.prune_batch == 0),
        ] {
            if zero {
                return Err(Error::Value {
                    key: key.to_owned(),
                    rule: "it must be above 0",
                });
            }
        }
        Ok(Retention {
            keep: Duration::from_hours(u64::from(self.retention_days).saturating_mul(24)),
            batch: self.prune_batch,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Outbox;
    use crate::config::error::Error;

    #[test]
    fn thirty_days_by_default_and_a_zero_bound_is_refused() {
        let resolved = Outbox::default().resolve().expect("the default resolves");
        assert_eq!(Duration::from_hours(30 * 24), resolved.keep);
        assert_eq!(500, resolved.batch);
        let zero = Outbox {
            retention_days: 0,
            ..Outbox::default()
        };
        assert!(
            matches!(zero.resolve(), Err(Error::Value { key, .. }) if key == "outbox.retention_days")
        );
    }
}
