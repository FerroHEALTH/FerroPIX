// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The duplicate-key check every answer passes before it is read.
//!
//! RFC 8259 §4 leaves the meaning of an object with a repeated name to each
//! parser, so two readers of one answer can take different values from it:
//! an endpoint the client checks and one it sends to. An answer that repeats
//! a name at any depth is refused instead (no specification governs this:
//! our own design).

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

/// A JSON value read only to prove that no object in it repeats a name.
pub(super) struct Unique;

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(UniqueVisitor)
    }
}

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = Unique;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value whose objects repeat no name")
    }

    fn visit_bool<E: de::Error>(self, _value: bool) -> Result<Unique, E> {
        Ok(Unique)
    }

    fn visit_i64<E: de::Error>(self, _value: i64) -> Result<Unique, E> {
        Ok(Unique)
    }

    fn visit_u64<E: de::Error>(self, _value: u64) -> Result<Unique, E> {
        Ok(Unique)
    }

    fn visit_f64<E: de::Error>(self, _value: f64) -> Result<Unique, E> {
        Ok(Unique)
    }

    fn visit_str<E: de::Error>(self, _value: &str) -> Result<Unique, E> {
        Ok(Unique)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
        Ok(Unique)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Unique, A::Error> {
        while seq.next_element::<Unique>()?.is_some() {}
        Ok(Unique)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Unique, A::Error> {
        let mut names = BTreeSet::new();
        while let Some(name) = map.next_key::<String>()? {
            if !names.insert(name) {
                return Err(de::Error::custom("an object repeats a name"));
            }
            map.next_value::<Unique>()?;
        }
        Ok(Unique)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_name_at_any_depth_is_refused() {
        assert!(serde_json::from_str::<Unique>(r#"{"a": 1, "b": {"c": [1, {"d": 2}]}}"#).is_ok());
        assert!(serde_json::from_str::<Unique>(r#"{"a": 1, "a": 2}"#).is_err());
        assert!(serde_json::from_str::<Unique>(r#"{"a": {"b": 1, "b": 1}}"#).is_err());
        assert!(serde_json::from_str::<Unique>(r#"[{"b": 1, "b": 1}]"#).is_err());
    }
}
