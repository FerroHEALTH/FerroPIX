// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The reveal, `POST {base}/operator/persons/{id}/reveal`: one identifier or
//! pseudonym of the person unmasked, with the steward's reason.
//!
//! A reveal is a privacy disclosure: the server records it as a steward
//! action and a BALP disclosure record before it answers the value, once,
//! never kept by a cache; a second look is a second reveal.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The seconds a revealed value is shown when the request names none.
pub const DEFAULT_SECONDS: u32 = 15;

/// The most seconds a revealed value is shown, whatever the request asks.
pub const MAX_SECONDS: u32 = 30;

/// The most characters a reason holds.
pub const MAX_REASON: usize = 500;

/// What a reveal unmasks, by the handle the person document gave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    /// The identifier of the source record with this id.
    Record(String),
    /// The carried authoritative identifier with this id.
    Carried(String),
    /// The person's pseudonym in the pseudonymisation domain with this id.
    Pseudonym(String),
}

/// The body of `POST {base}/operator/persons/{id}/reveal`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reveal {
    /// What to unmask.
    pub target: Target,
    /// Why, in the steward's words: required, at most [`MAX_REASON`]
    /// characters.
    pub reason: String,
    /// The seconds the console shows the value, at most [`MAX_SECONDS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<u32>,
}

impl fmt::Debug for Reveal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reveal")
            .field("target", &self.target)
            .field("reason", &self.reason.chars().count())
            .field("seconds", &self.seconds)
            .finish()
    }
}

/// A revealed value, which no `Debug` rendering shows.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Value(pub String);

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Value(***)")
    }
}

/// What a reveal answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revealed {
    /// The value, unmasked.
    pub value: Value,
    /// The identifier's domain system, or the pseudonymisation domain's
    /// namespace.
    pub system: String,
    /// The seconds the console shows the value before it masks it again.
    pub seconds: u32,
    /// The id of the steward action that records the disclosure.
    pub action: String,
}

#[cfg(test)]
mod tests {
    use super::{Reveal, Revealed, Target, Value};

    #[test]
    fn a_reveal_shows_neither_its_reason_nor_its_value_in_a_debug_rendering() {
        let reveal = Reveal {
            target: Target::Pseudonym(String::from("d-1")),
            reason: String::from("Synthfamily asked by phone"),
            seconds: Some(10),
        };
        let revealed = Revealed {
            value: Value(String::from("SYNTHETIC-4711")),
            system: String::from("urn:oid:2.999.1.1"),
            seconds: 10,
            action: String::from("a-1"),
        };
        let shown = format!("{reveal:?} {revealed:?}");
        assert!(!shown.contains("Synthfamily"), "{shown}");
        assert!(!shown.contains("SYNTHETIC-4711"), "{shown}");
        let text = serde_json::to_string(&reveal).expect("it serializes");
        assert_eq!(
            r#"{"target":{"pseudonym":"d-1"},"reason":"Synthfamily asked by phone","seconds":10}"#,
            text
        );
    }
}
