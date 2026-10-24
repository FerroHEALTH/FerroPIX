// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why a configuration is refused.
//!
//! Every variant names the key that carries the fault and never the value, so
//! a refusal printed on stderr or in a log shows no secret. A parse refusal
//! keeps the parser's message and its position and drops the source line the
//! parser would quote. No specification governs this: our own design.

use std::fmt;
use std::path::PathBuf;

/// A configuration that does not load.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The named configuration file cannot be read.
    #[error("cannot read the configuration file {}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// What the file system reported.
        #[source]
        source: std::io::Error,
    },
    /// The text does not read as the configuration tree: bad TOML, an unknown
    /// key, or a value of the wrong type.
    #[error("{fault}")]
    Parse {
        /// The parser's message and where it points.
        fault: ParseFault,
    },
    /// An environment variable under the prefix names no section and key.
    #[error("the environment variable {name} names no section and key")]
    EnvName {
        /// The variable.
        name: String,
    },
    /// An environment variable addresses a key under a value that is no table.
    #[error("the environment variable {name} addresses a key under a value that is no table")]
    EnvShape {
        /// The variable.
        name: String,
    },
    /// The tree merged from the file and the environment cannot be written.
    #[error("the configuration merged from the file and the environment cannot be written")]
    Assemble {
        /// What the writer reported.
        #[source]
        source: toml::ser::Error,
    },
    /// A secret is set under its key and under its `_file` sibling.
    #[error("{key} and {key}_file are both set; set one")]
    SecretConflict {
        /// The key of the secret.
        key: String,
    },
    /// A secret is set under its key while `profile = "production"`.
    #[error("{key} is set inline, which profile = \"production\" refuses; set {key}_file")]
    InlineSecret {
        /// The key of the secret.
        key: String,
    },
    /// A secret file cannot be read.
    #[error("{key} names {}, which cannot be read", path.display())]
    SecretFile {
        /// The `_file` key.
        key: String,
        /// The file.
        path: PathBuf,
        /// What the file system reported.
        #[source]
        source: std::io::Error,
    },
    /// A secret file holds only whitespace.
    #[error("{key} names {}, which is empty", path.display())]
    EmptySecret {
        /// The `_file` key.
        key: String,
        /// The file.
        path: PathBuf,
    },
    /// A key the server needs is not set.
    #[error("{key} is not set")]
    Missing {
        /// The key.
        key: String,
    },
    /// A socket address does not parse.
    #[error("{key} is not a socket address")]
    Listen {
        /// The key.
        key: String,
        /// What the parser reported.
        #[source]
        source: std::net::AddrParseError,
    },
    /// A URL does not parse.
    #[error("{key} is not a URL")]
    Url {
        /// The key.
        key: String,
        /// What the parser reported.
        #[source]
        source: url::ParseError,
    },
    /// A value is outside the rule its key has.
    #[error("{key} is refused: {rule}")]
    Value {
        /// The key.
        key: String,
        /// The rule the value breaks.
        rule: &'static str,
    },
    /// The `[dev]` table is set while `profile = "production"`.
    #[error("the [dev] table is set, which profile = \"production\" refuses")]
    DevUnderProduction,
    /// A setting only the development profile admits is set under production.
    #[error("{key} is admitted under profile = \"development\" only: {rule}")]
    DevelopmentOnly {
        /// The key.
        key: String,
        /// What makes the setting a development one.
        rule: &'static str,
    },
    /// A setting only the `[dev]` table admits is set without it.
    #[error("{key} is admitted only beside the [dev] table")]
    DevTableRequired {
        /// The key.
        key: String,
    },
    /// `[dev] loopback` holds and a listener binds an address that is no
    /// loopback address.
    #[error("{key} binds an address that is no loopback address, which [dev] loopback refuses")]
    NotLoopback {
        /// The key.
        key: String,
    },
    /// The `[auth]` section breaks a rule of the resource-server crate.
    #[error("the [auth] section is refused")]
    Auth {
        /// The rule.
        #[source]
        source: ferrohealth_smart::config::ConfigError,
    },
    /// A `[[domain]]` names a Source client no issuer admits.
    #[error(
        "domain[{index}].source_client_id names {client:?}, which no [[auth.issuer]] lists in its clients"
    )]
    ClientNotAdmitted {
        /// The position of the entry in the seed, from 0.
        index: usize,
        /// The client id.
        client: String,
    },
    /// Two `[[domain]]` entries name one system.
    #[error("domain[{index}].system names {system:?}, which an earlier entry seeds already")]
    DuplicateSystem {
        /// The position of the later entry, from 0.
        index: usize,
        /// The system.
        system: String,
    },
    /// Two `[[hl7v2.senders]]` entries name one sending application and
    /// facility.
    #[error(
        "hl7v2.senders[{index}] names a sending application and facility an earlier entry names"
    )]
    DuplicateSender {
        /// The position of the later entry, from 0.
        index: usize,
    },
}

/// Which text a parse refusal points into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The configuration file as written.
    File,
    /// The tree after the environment overrides were applied.
    Merged,
}

/// The parser's message and the position it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseFault {
    /// The parser's message, which names the key and never quotes the line.
    pub message: String,
    /// The line and column, from 1, when the parser gave a position.
    pub position: Option<(usize, usize)>,
    /// The text the position points into.
    pub stage: Stage,
}

impl ParseFault {
    /// Returns the fault `error` describes, with its position read against
    /// `text` when the text is the one the parser read.
    #[must_use]
    pub fn from_toml(error: &toml::de::Error, text: Option<&str>, stage: Stage) -> Self {
        let position = text
            .zip(error.span())
            .map(|(text, span)| line_and_column(text, span.start));
        Self {
            message: error.message().trim().to_owned(),
            position,
            stage,
        }
    }
}

impl fmt::Display for ParseFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.stage, self.position) {
            (Stage::File, Some((line, column))) => write!(
                f,
                "the configuration file does not read at line {line}, column {column}: {}",
                self.message
            ),
            (Stage::File, None) => {
                write!(f, "the configuration file does not read: {}", self.message)
            }
            (Stage::Merged, _) => write!(
                f,
                "the configuration does not read after the environment overrides: {}",
                self.message
            ),
        }
    }
}

/// Returns the line and column, from 1, of the byte `offset` into `text`.
fn line_and_column(text: &str, offset: usize) -> (usize, usize) {
    let before = text.get(..offset).unwrap_or(text);
    let line = before.matches('\n').count().saturating_add(1);
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |last| last.chars().count())
        .saturating_add(1);
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::{ParseFault, Stage, line_and_column};

    #[test]
    fn a_position_is_counted_in_lines_and_characters_from_one() {
        assert_eq!((1, 1), line_and_column("abc", 0));
        assert_eq!((2, 3), line_and_column("a\nbcd", 4));
        assert_eq!(
            (1, 2),
            line_and_column("\u{e9}x", 2),
            "a character, not a byte"
        );
    }

    #[test]
    fn a_parse_fault_names_the_key_and_never_quotes_the_line() {
        let text = "[db]\nurl = \"postgres://u:Qz7pw@db.invalid/pix\"\npasword = \"Qz7pw\"\n";
        let error = toml::from_str::<toml::Table>(text)
            .and_then(toml::Table::try_into::<Strict>)
            .expect_err("an unknown key is refused");
        let fault = ParseFault::from_toml(&error, None, Stage::Merged);
        let shown = fault.to_string();
        assert!(!shown.contains("Qz7pw"), "{shown}");
    }

    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    #[expect(dead_code, reason = "the struct exists to be refused")]
    struct Strict {
        db: StrictDb,
    }

    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    #[expect(dead_code, reason = "the struct exists to be refused")]
    struct StrictDb {
        url: String,
    }
}
