// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Where each setting came from: the file, the environment, or neither.
//!
//! The operator console draws the configuration read-only, the file named
//! once as where it is set, and marks a value the environment set ("From the
//! environment") or nothing set ("Default"), as FerroUI 0.3.7
//! `design/COMPONENTS.md` rule 13 and its setting rows ask of a product
//! that keeps no configuration store. Every leaf key of the file and every
//! `FERROPIX__` override is recorded by its dotted path. No specification
//! governs this: our own design.

use std::collections::BTreeMap;

use serde::Serialize;

/// Where one setting's value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The configuration file sets it.
    File,
    /// A `FERROPIX__` variable sets it, over the file.
    Environment,
    /// Nothing sets it: the built-in default.
    Default,
}

/// The origin of every setting the file or the environment set, and the
/// name of the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origins {
    /// The configuration file's name, without its directory, when one was
    /// read.
    file: Option<String>,
    /// The origin of each dotted key the file or the environment set.
    keys: BTreeMap<String, Origin>,
}

impl Origins {
    /// Returns the origins of the leaves of `file`, then of the overrides
    /// `environment` names by their dotted keys, which win.
    #[must_use]
    pub fn of(file: &toml::Table, environment: &[String]) -> Self {
        let mut keys = BTreeMap::new();
        leaves(file, "", &mut keys);
        for key in environment {
            keys.insert(key.clone(), Origin::Environment);
        }
        Self { file: None, keys }
    }

    /// Returns these origins with the file named `name`.
    #[must_use]
    pub fn with_file(mut self, name: Option<String>) -> Self {
        self.file = name;
        self
    }

    /// Returns the configuration file's name, `None` when no file was read.
    #[must_use]
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// Returns where the setting at `key`, a dotted path, came from: its own
    /// origin, or the origin of the table above it that the environment set.
    #[must_use]
    pub fn origin(&self, key: &str) -> Origin {
        if let Some(origin) = self.keys.get(key) {
            return *origin;
        }
        let mut prefix = key;
        while let Some((parent, _)) = prefix.rsplit_once('.') {
            if let Some(origin) = self.keys.get(parent) {
                return *origin;
            }
            prefix = parent;
        }
        Origin::Default
    }
}

/// Records every leaf of `table` under `prefix` as set by the file.
fn leaves(table: &toml::Table, prefix: &str, keys: &mut BTreeMap<String, Origin>) {
    for (name, value) in table {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            toml::Value::Table(inner) => leaves(inner, &key, keys),
            _ => {
                keys.insert(key, Origin::File);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Origin, Origins};

    #[test]
    fn a_key_is_from_the_file_the_environment_or_the_default() {
        let file: toml::Table =
            toml::from_str("[matching]\nlink_at = 0.9\n[matching.fields.gender]\nm = 0.9\n")
                .expect("TOML");
        let origins = Origins::of(&file, &[String::from("matching.review_at")])
            .with_file(Some(String::from("ferropix.toml")));
        assert_eq!(origins.origin("matching.link_at"), Origin::File);
        assert_eq!(origins.origin("matching.fields.gender.m"), Origin::File);
        assert_eq!(origins.origin("matching.review_at"), Origin::Environment);
        assert_eq!(origins.origin("matching.auto_link"), Origin::Default);
        assert_eq!(origins.file(), Some("ferropix.toml"));
    }
}
