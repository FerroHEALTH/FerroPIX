// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The fixture guard: a fixture holds synthetic values only.
//!
//! [`check`] walks a JSON document and refuses
//!
//! - a BSN (an `Identifier` under one of the BSN naming systems, or a
//!   `system|value` token naming one) outside the synthetic range or failing
//!   the eleven-proof;
//! - any other nine-digit string that passes the eleven-proof outside the
//!   synthetic range, a BSN under a system that does not say so;
//! - a `HumanName` whose `family`, `given` or `text` holds a name outside
//!   the synthetic set;
//! - an `Identifier` in a domain that is neither a synthetic domain nor the
//!   BSN; and
//! - a narrative, which the guard cannot read.
//!
//! A refusal names the element path and never the value it refused, so a
//! real identifier that slipped into a fixture does not reach a test log.
//! [`check_tree`] applies the guard to every file under a directory, so a
//! new fixture is held to it the moment it is committed.

use crate::synthetic::is_synthetic_bsn;
use crate::synthetic::{FAMILY_NAMES, GIVEN_NAMES, SYNTHETIC_DOMAINS, is_eleven_proof};
use fhir_types::codec::{Object, Value};
use nl_generic_functions::identification::is_bsn_system;
use std::fmt;
use std::path::{Path, PathBuf};

/// One value a fixture may not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Violation {
    /// A BSN outside the synthetic range or failing the eleven-proof.
    BsnOutsideRange {
        /// The element path of the value.
        path: String,
    },
    /// A nine-digit string that passes the eleven-proof outside the
    /// synthetic range, under no BSN system.
    UndeclaredBsn {
        /// The element path of the value.
        path: String,
    },
    /// A name outside the synthetic set.
    NameOutsideSet {
        /// The element path of the name part.
        path: String,
    },
    /// An identifier in a domain that is neither synthetic nor the BSN.
    UnknownDomain {
        /// The element path of the identifier.
        path: String,
        /// The domain it names, which identifies no one.
        system: String,
    },
    /// A narrative, which the guard cannot read.
    Narrative {
        /// The element path of the narrative.
        path: String,
    },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BsnOutsideRange { path } => {
                write!(f, "{path}: a BSN outside the synthetic range")
            }
            Self::UndeclaredBsn { path } => {
                write!(f, "{path}: a valid BSN outside the synthetic range")
            }
            Self::NameOutsideSet { path } => write!(f, "{path}: a name outside the synthetic set"),
            Self::UnknownDomain { path, system } => {
                write!(f, "{path}: an identifier in the unknown domain {system}")
            }
            Self::Narrative { path } => write!(f, "{path}: a narrative the guard cannot read"),
        }
    }
}

/// A file under a fixture tree could not be checked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TreeError {
    /// A directory or file could not be read.
    #[error("cannot read {}", path.display())]
    Read {
        /// The directory or file.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A file is not JSON.
    #[error("{} is not JSON", path.display())]
    Json {
        /// The file.
        path: PathBuf,
        /// What the JSON reader reported.
        #[source]
        source: serde_json::Error,
    },
}

/// Returns every violation in `document`, in document order; an empty list
/// means the document holds synthetic values only.
///
/// # Examples
///
/// ```
/// use ferropix_testkit::fixtures::Fixture;
/// use ferropix_testkit::guard::check;
///
/// let document = Fixture::FeedHospital.document()?;
/// assert!(check(&document).is_empty());
/// # Ok::<(), ferropix_testkit::fixtures::FixtureError>(())
/// ```
#[must_use]
pub fn check(document: &Value) -> Vec<Violation> {
    let mut violations = Vec::new();
    walk(document, "$", None, &mut violations);
    violations
}

/// Checks every `.json` file under `root`, recursively, and returns each
/// file with its violations, files with none left out, in path order.
///
/// # Errors
///
/// Returns [`TreeError::Read`] when a directory or file cannot be read and
/// [`TreeError::Json`] when a file is not JSON.
pub fn check_tree(root: &Path) -> Result<Vec<(PathBuf, Vec<Violation>)>, TreeError> {
    let mut found = Vec::new();
    for file in json_files(root)? {
        let text = std::fs::read_to_string(&file).map_err(|source| TreeError::Read {
            path: file.clone(),
            source,
        })?;
        let document: Value = serde_json::from_str(&text).map_err(|source| TreeError::Json {
            path: file.clone(),
            source,
        })?;
        let violations = check(&document);
        if !violations.is_empty() {
            found.push((file, violations));
        }
    }
    Ok(found)
}

/// Returns every `.json` file under `root`, recursively, sorted.
///
/// # Errors
///
/// Returns [`TreeError::Read`] when a directory cannot be read.
pub fn json_files(root: &Path) -> Result<Vec<PathBuf>, TreeError> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory).map_err(|source| TreeError::Read {
            path: directory.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| TreeError::Read {
                path: directory.clone(),
                source,
            })?;
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Walks `value` at `path`, reached under the member `key`.
fn walk(value: &Value, path: &str, key: Option<&str>, out: &mut Vec<Violation>) {
    match value {
        Value::Object(object) => {
            check_object(object, path, key, out);
            let checked_bsn = is_identifier(key)
                && object
                    .get("system")
                    .and_then(Value::as_str)
                    .is_some_and(is_bsn_system);
            for (member, child) in object {
                if checked_bsn && member == "value" {
                    continue;
                }
                walk(child, &format!("{path}.{member}"), Some(member), out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk(item, &format!("{path}[{index}]"), key, out);
            }
        }
        Value::String(text) => check_string(text, path, out),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Returns whether an object reached under `key` is an `Identifier`.
fn is_identifier(key: Option<&str>) -> bool {
    matches!(key, Some("identifier" | "valueIdentifier"))
}

/// Checks one object by the member it was reached under.
fn check_object(object: &Object, path: &str, key: Option<&str>, out: &mut Vec<Violation>) {
    if key == Some("text") && object.contains_key("div") {
        out.push(Violation::Narrative {
            path: path.to_owned(),
        });
    }
    if is_identifier(key) {
        check_identifier(object, path, out);
    } else if matches!(key, Some("name" | "valueHumanName")) {
        check_name(object, path, out);
    }
}

/// Checks an `Identifier`: its domain, and its value when it is a BSN.
fn check_identifier(object: &Object, path: &str, out: &mut Vec<Violation>) {
    let system = object.get("system").and_then(Value::as_str);
    let value = object.get("value").and_then(Value::as_str);
    match system {
        Some(system) if is_bsn_system(system) => {
            if !value.is_some_and(is_synthetic_bsn) {
                out.push(Violation::BsnOutsideRange {
                    path: format!("{path}.value"),
                });
            }
        }
        Some(system) if SYNTHETIC_DOMAINS.contains(&system) => {}
        Some(system) => out.push(Violation::UnknownDomain {
            path: path.to_owned(),
            system: system.to_owned(),
        }),
        None => {}
    }
}

/// Checks a `HumanName`: every family, given and text word in the set.
fn check_name(object: &Object, path: &str, out: &mut Vec<Violation>) {
    if let Some(family) = object.get("family").and_then(Value::as_str)
        && !FAMILY_NAMES.contains(&family)
    {
        out.push(Violation::NameOutsideSet {
            path: format!("{path}.family"),
        });
    }
    for (index, given) in object
        .get("given")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        if !given
            .as_str()
            .is_some_and(|name| GIVEN_NAMES.contains(&name))
        {
            out.push(Violation::NameOutsideSet {
                path: format!("{path}.given[{index}]"),
            });
        }
    }
    if let Some(text) = object.get("text").and_then(Value::as_str)
        && !text
            .split_whitespace()
            .all(|word| FAMILY_NAMES.contains(&word) || GIVEN_NAMES.contains(&word))
    {
        out.push(Violation::NameOutsideSet {
            path: format!("{path}.text"),
        });
    }
}

/// Checks a string: a `system|value` token naming the BSN, and a bare
/// number that would pass for one.
fn check_string(text: &str, path: &str, out: &mut Vec<Violation>) {
    if let Some((system, value)) = text.split_once('|')
        && is_bsn_system(system)
    {
        if !is_synthetic_bsn(value) {
            out.push(Violation::BsnOutsideRange {
                path: path.to_owned(),
            });
        }
        return;
    }
    if is_eleven_proof(text) && !is_synthetic_bsn(text) {
        out.push(Violation::UndeclaredBsn {
            path: path.to_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Violation, check};
    use fhir_types::codec::Value;

    fn document(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn a_synthetic_patient_passes() {
        let patient = document(
            r#"{"resourceType":"Patient","identifier":[{"system":"urn:oid:2.999.1.1","value":"H-1"}],
            "name":[{"family":"Proefsma","given":["Ada"],"text":"Ada Proefsma"}]}"#,
        );
        assert_eq!(Vec::<Violation>::new(), check(&patient));
    }

    #[test]
    fn a_parameter_name_is_not_a_human_name() {
        let parameters = document(
            r#"{"resourceType":"Parameters","parameter":[{"name":"targetSystem","valueString":"urn:oid:2.999.1.1"}]}"#,
        );
        assert_eq!(Vec::<Violation>::new(), check(&parameters));
    }

    #[test]
    fn a_narrative_is_refused() {
        let patient = document(
            r#"{"resourceType":"Patient","text":{"status":"generated","div":"<div xmlns=\"http://www.w3.org/1999/xhtml\"/>"}}"#,
        );
        assert_eq!(
            vec![Violation::Narrative {
                path: "$.text".to_owned()
            }],
            check(&patient)
        );
    }

    #[test]
    fn an_unknown_domain_is_refused() {
        let patient = document(
            r#"{"resourceType":"Patient","identifier":[{"system":"urn:oid:1.2.3","value":"X"}]}"#,
        );
        assert_eq!(
            vec![Violation::UnknownDomain {
                path: "$.identifier[0]".to_owned(),
                system: "urn:oid:1.2.3".to_owned()
            }],
            check(&patient)
        );
    }

    #[test]
    fn a_refusal_never_prints_the_value() {
        let outside = (100_000_000_u32..=100_000_099)
            .map(|number| number.to_string())
            .find(|digits| super::is_eleven_proof(digits))
            .unwrap();
        let patient = document(&format!(
            r#"{{"resourceType":"Patient","identifier":[{{"system":"http://fhir.nl/fhir/NamingSystem/bsn","value":"{outside}"}}]}}"#
        ));
        let violations = check(&patient);
        assert_eq!(1, violations.len(), "{violations:?}");
        for violation in violations {
            assert!(!violation.to_string().contains(&outside), "{violation}");
            assert!(
                !format!("{violation:?}").contains(&outside),
                "{violation:?}"
            );
        }
    }
}
