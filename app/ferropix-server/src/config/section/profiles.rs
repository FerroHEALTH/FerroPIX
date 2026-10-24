// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[pixm]`, `[pdqm]` and `[pmir]` sections: each profile's switch and
//! the options its text leaves to the actor.

use serde::{Deserialize, Serialize};

use crate::config::error::Error;

/// The `[pixm]` section: the PIXm Patient Identifier Cross-reference Manager.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pixm {
    /// Whether ITI-83 and ITI-104 are served.
    pub enabled: bool,
    /// Whether the Remove Patient Option of ITI-104 is served (PIXm 3.1.0
    /// §1:41.2.1, §2:3.104.4.3).
    pub remove_patient: bool,
}

impl Default for Pixm {
    fn default() -> Self {
        Self {
            enabled: true,
            remove_patient: false,
        }
    }
}

/// How a merged person is answered, among the choices PDQm 3.2.0
/// §2:3.78.4.1.3 case 6 leaves to the Supplier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Deprecated {
    /// The person with `active = false` and its `replaced-by` link, the
    /// survivor included.
    #[default]
    Inactive,
    /// The person left out of the answer.
    Omit,
}

/// The `[pdqm]` section: the PDQm Patient Demographics Supplier.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pdqm {
    /// Whether ITI-78 and ITI-119 are served.
    pub enabled: bool,
    /// How a merged person is answered.
    pub deprecated: Deprecated,
}

impl Default for Pdqm {
    fn default() -> Self {
        Self {
            enabled: true,
            deprecated: Deprecated::Inactive,
        }
    }
}

/// The `[pmir.feed]` table: the retry bounds of the outbound ITI-93 sender,
/// after which a subscription's `status` becomes `error` (PMIR 1.6.0
/// §2:3.94.4.1.3).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Feed {
    /// The most attempts one message is given.
    pub max_attempts: u32,
    /// The wait before the first retry.
    pub initial_backoff_ms: u64,
    /// The longest wait between two retries.
    pub max_backoff_ms: u64,
}

impl Default for Feed {
    fn default() -> Self {
        Self {
            max_attempts: 10,
            initial_backoff_ms: 1_000,
            max_backoff_ms: 300_000,
        }
    }
}

/// The `[pmir]` section: the PMIR Patient Identity Registry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pmir {
    /// Whether ITI-93 and ITI-94 are served.
    pub enabled: bool,
    /// The retry bounds of the outbound ITI-93 sender.
    pub feed: Feed,
}

impl Pmir {
    /// Checks the section and returns it as resolved.
    ///
    /// # Errors
    /// Returns [`Error::Value`] for no attempt, a first wait of 0 or a first
    /// wait above the longest one.
    pub fn resolve(&self) -> Result<Self, Error> {
        // NOTE: no specification governs this: our own design; PMIR 1.6.0
        // §2:3.94.4.1.3 leaves the retry policy to the Registry.
        let rule = if self.feed.max_attempts == 0 {
            Some(("pmir.feed.max_attempts", "it must be above 0"))
        } else if self.feed.initial_backoff_ms == 0 {
            Some(("pmir.feed.initial_backoff_ms", "it must be above 0"))
        } else if self.feed.initial_backoff_ms > self.feed.max_backoff_ms {
            Some((
                "pmir.feed.max_backoff_ms",
                "it must be at least pmir.feed.initial_backoff_ms",
            ))
        } else {
            None
        };
        match rule {
            Some((key, rule)) => Err(Error::Value {
                key: key.to_owned(),
                rule,
            }),
            None => Ok(self.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Feed, Pmir};
    use crate::config::error::Error;

    #[test]
    fn retry_bounds_without_an_attempt_or_out_of_order_are_refused() {
        for feed in [
            Feed {
                max_attempts: 0,
                ..Feed::default()
            },
            Feed {
                initial_backoff_ms: 0,
                ..Feed::default()
            },
            Feed {
                initial_backoff_ms: 10,
                max_backoff_ms: 5,
                ..Feed::default()
            },
        ] {
            let pmir = Pmir {
                enabled: true,
                feed: feed.clone(),
            };
            assert!(
                matches!(pmir.resolve(), Err(Error::Value { .. })),
                "{feed:?}"
            );
        }
        Pmir::default().resolve().expect("the defaults resolve");
    }
}
