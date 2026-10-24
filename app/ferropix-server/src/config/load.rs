// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Reading the configuration tree: the TOML file, then every `FERROPIX__`
//! override applied over it.
//!
//! An environment variable carries text, and the tree decides what the text
//! is. Every override is first placed as the string it is, so a string key
//! takes `12345` or `true` verbatim; when the tree then refuses a string where
//! it wants a number, a boolean or an array and the text reads as that TOML
//! value, the override is placed typed and the tree is read again. No
//! specification governs this: our own design, after FerroBRIDGE's loader.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::config::error::{Error, ParseFault, Stage};
use crate::config::{CONFIG_PATH_ENV, Config, ENV_PREFIX};

/// One `FERROPIX__` variable, with the key it addresses.
#[derive(Debug)]
struct Override {
    /// The variable name, for a refusal.
    name: String,
    /// The lower-case key segments, the section first.
    path: Vec<String>,
    /// The text the variable carries.
    raw: String,
    /// The text read as a TOML value that is not a string, when it reads so.
    typed: Option<toml::Value>,
}

impl Config {
    /// Reads the configuration file `path` names, then the environment.
    ///
    /// `path` is what `--config` named; without it the file is the one
    /// `FERROPIX_CONFIG` names, and without that the defaults stand.
    ///
    /// # Errors
    /// Returns [`Error::Read`] when a named file cannot be read and every
    /// other [`Error`] [`Config::from_sources`] returns.
    pub fn load(path: Option<&Path>) -> Result<Self, Error> {
        let named = path
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os(CONFIG_PATH_ENV).map(PathBuf::from));
        let text = match &named {
            None => None,
            Some(path) => Some(std::fs::read_to_string(path).map_err(|source| Error::Read {
                path: path.clone(),
                source,
            })?),
        };
        let environment: BTreeMap<String, String> = std::env::vars()
            .filter(|(name, _)| name.starts_with(ENV_PREFIX))
            .collect();
        Self::from_sources(text.as_deref(), &environment)
    }

    /// Reads `text` as TOML and applies `environment` over it.
    ///
    /// Reading the sources and reading the process environment are separate
    /// so a test can state both: setting an environment variable is `unsafe`
    /// in edition 2024 and this workspace forbids `unsafe`.
    ///
    /// # Errors
    /// Returns [`Error::Parse`] for text that does not read as this tree,
    /// [`Error::EnvName`] and [`Error::EnvShape`] for an override that names
    /// no key, and [`Error::Assemble`] when the merged tree cannot be written.
    pub fn from_sources(
        text: Option<&str>,
        environment: &BTreeMap<String, String>,
    ) -> Result<Self, Error> {
        let file = match text {
            None => toml::Table::new(),
            Some(text) => toml::from_str(text).map_err(|error| Error::Parse {
                fault: ParseFault::from_toml(&error, Some(text), Stage::File),
            })?,
        };
        let mut overrides = Vec::new();
        for (name, raw) in environment {
            if let Some(item) = parse_override(name, raw)? {
                overrides.push(item);
            }
        }
        // Each pass places one more override typed, so the loop ends after at
        // most one pass per override.
        let mut typed = BTreeSet::new();
        loop {
            let mut table = file.clone();
            for (index, item) in overrides.iter().enumerate() {
                apply_override(&mut table, item, typed.contains(&index))?;
            }
            let merged = toml::to_string(&table).map_err(|source| Error::Assemble { source })?;
            let error = match toml::from_str::<Self>(&merged) {
                Ok(config) => return Ok(config),
                Err(error) => error,
            };
            let retyped = error
                .span()
                .and_then(|span| retype(&overrides, &typed, &merged, &span));
            if let Some(index) = retyped {
                typed.insert(index);
                continue;
            }
            // NOTE: no specification governs this: our own design; a fault the
            // file itself carries is reported at its own line.
            let fault = text
                .and_then(|text| {
                    toml::from_str::<Self>(text)
                        .err()
                        .map(|error| ParseFault::from_toml(&error, Some(text), Stage::File))
                })
                .unwrap_or_else(|| ParseFault::from_toml(&error, None, Stage::Merged));
            return Err(Error::Parse { fault });
        }
    }
}

/// Returns the override `name` makes, or `None` for a variable outside the
/// prefix.
fn parse_override(name: &str, raw: &str) -> Result<Option<Override>, Error> {
    let Some(rest) = name.strip_prefix(ENV_PREFIX) else {
        return Ok(None);
    };
    let path: Vec<String> = rest.split("__").map(str::to_ascii_lowercase).collect();
    if path.len() < 2 || path.iter().any(String::is_empty) {
        return Err(Error::EnvName {
            name: name.to_owned(),
        });
    }
    Ok(Some(Override {
        name: name.to_owned(),
        path,
        raw: raw.to_owned(),
        typed: typed_value(raw),
    }))
}

/// Applies `item` onto `table`, typed or as its text.
fn apply_override(table: &mut toml::Table, item: &Override, typed: bool) -> Result<(), Error> {
    let Some((key, parents)) = item.path.split_last() else {
        return Err(Error::EnvName {
            name: item.name.clone(),
        });
    };
    let mut cursor = table;
    for parent in parents {
        let entry = cursor
            .entry(parent.clone())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let toml::Value::Table(next) = entry else {
            return Err(Error::EnvShape {
                name: item.name.clone(),
            });
        };
        cursor = next;
    }
    let value = match (&item.typed, typed) {
        (Some(value), true) => value.clone(),
        _ => toml::Value::String(item.raw.clone()),
    };
    cursor.insert(key.clone(), value);
    Ok(())
}

/// Returns the index of the override the tree refused as a string and that
/// reads as another TOML value, so it is placed typed on the next read.
fn retype(
    overrides: &[Override],
    typed: &BTreeSet<usize>,
    merged: &str,
    refused: &Range<usize>,
) -> Option<usize> {
    // NOTE: no specification governs this: our own design; a merged text that
    // does not parse leaves the refusal standing as the parser reported it.
    let document = toml::de::DeTable::parse(merged).ok()?.into_inner();
    overrides.iter().enumerate().find_map(|(index, item)| {
        if typed.contains(&index) || item.typed.is_none() {
            return None;
        }
        let span = value_span(&document, &item.path)?;
        (span.start <= refused.start && refused.end <= span.end).then_some(index)
    })
}

/// Returns the span of the value `path` addresses in `document`.
fn value_span(document: &toml::de::DeTable<'_>, path: &[String]) -> Option<Range<usize>> {
    let (first, rest) = path.split_first()?;
    let mut value = document.get(first.as_str())?;
    for segment in rest {
        value = value.get_ref().get(segment.as_str())?;
    }
    Some(value.span())
}

/// Reads `raw` as a TOML value that is not a string: a number, a boolean, a
/// date-time, an array or an inline table.
fn typed_value(raw: &str) -> Option<toml::Value> {
    // NOTE: no specification governs this: our own design; a parse failure IS
    // the answer here, because text that is not TOML syntax stays a string.
    let parsed = toml::from_str::<toml::Table>(&format!("value = {raw}"))
        .ok()
        .and_then(|table| table.get("value").cloned())?;
    (!parsed.is_str()).then_some(parsed)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{parse_override, typed_value};
    use crate::config::Config;
    use crate::config::error::{Error, Stage};

    fn environment(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn an_environment_value_reads_typed_only_as_toml_syntax_that_is_not_a_string() {
        assert_eq!(Some(toml::Value::Integer(5)), typed_value("5"));
        assert_eq!(Some(toml::Value::Boolean(true)), typed_value("true"));
        assert_eq!(None, typed_value("\"quoted\""));
        assert_eq!(None, typed_value("0.0.0.0:8080"));
        assert_eq!(None, typed_value("urn:oid:2.16.840.1.113883.2.4.6.3"));
    }

    #[test]
    fn an_override_name_without_a_section_and_a_key_is_refused() {
        for name in [
            "FERROPIX__LISTEN",
            "FERROPIX__SERVER__",
            "FERROPIX____LISTEN",
        ] {
            let error = parse_override(name, "x").expect_err("a section and a key are required");
            assert!(matches!(error, Error::EnvName { .. }), "{name}: {error:?}");
        }
        assert!(
            parse_override("OTHER__SERVER__LISTEN", "x")
                .expect("a variable outside the prefix is ignored")
                .is_none()
        );
    }

    #[test]
    fn an_override_wins_over_the_file_and_takes_the_type_its_key_wants() {
        let config = Config::from_sources(
            Some("[server]\nlisten = \"127.0.0.1:1\"\nbody_limit_bytes = 10\n"),
            &environment(&[
                ("FERROPIX__SERVER__LISTEN", "0.0.0.0:8080"),
                ("FERROPIX__SERVER__BODY_LIMIT_BYTES", "2048"),
                ("FERROPIX__db__migrate", "false"),
                ("FERROPIX__TELEMETRY__FILTER", "1"),
            ]),
        )
        .expect("the overrides apply");
        assert_eq!("0.0.0.0:8080", config.server.listen);
        assert_eq!(2048, config.server.body_limit_bytes);
        assert!(!config.db.migrate);
        assert_eq!("1", config.telemetry.filter, "a string key keeps the text");
    }

    #[test]
    fn an_override_under_a_value_that_is_not_a_table_is_refused() {
        let error = Config::from_sources(
            Some("[server]\nlisten = \"127.0.0.1:1\"\n"),
            &environment(&[("FERROPIX__SERVER__LISTEN__PORT", "1")]),
        )
        .expect_err("listen is a string, not a table");
        assert!(matches!(error, Error::EnvShape { .. }), "{error:?}");
    }

    #[test]
    fn an_unknown_key_in_the_file_is_refused_at_its_line() {
        let error = Config::from_sources(
            Some("profile = \"production\"\n\n[server]\nlisten_port = 8080\n"),
            &BTreeMap::new(),
        )
        .expect_err("an unknown key is refused");
        let Error::Parse { fault } = &error else {
            panic!("a parse refusal: {error:?}");
        };
        assert_eq!(Stage::File, fault.stage);
        assert_eq!(Some(4), fault.position.map(|(line, _)| line));
        assert!(fault.message.contains("listen_port"), "{fault}");
    }

    #[test]
    fn an_unknown_key_from_the_environment_is_refused_and_named() {
        let error = Config::from_sources(None, &environment(&[("FERROPIX__DB__PASWORD", "x")]))
            .expect_err("an unknown key is refused");
        let Error::Parse { fault } = &error else {
            panic!("a parse refusal: {error:?}");
        };
        assert_eq!(Stage::Merged, fault.stage);
        assert!(fault.message.contains("pasword"), "{fault}");
    }
}
