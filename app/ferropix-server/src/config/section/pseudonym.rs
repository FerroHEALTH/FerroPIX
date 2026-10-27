// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[[pseudonym_domain]]` seed: the pseudonymisation domains written to
//! the store at start when they are absent, never updated, so the database
//! stays the record.
//!
//! A pseudonymisation domain is a namespace, the system a person's pseudonym
//! carries in ITI-83 (PIXm 3.1.0 §2:3.83.4.1.2.2), and names the one client
//! a pseudonym is minted for when it asks. No specification governs the
//! seed: our own design.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::config::error::Error;
use crate::config::section::domain::{DomainSeed, is_system};

/// One `[[pseudonym_domain]]` entry as written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PseudonymDomain {
    /// The namespace: an absolute URI, the system of the pseudonyms.
    pub namespace: String,
    /// The name a steward reads.
    pub name: String,
    /// The client id a pseudonym is minted for in ITI-83, when one is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer_client_id: Option<String>,
}

/// One seeded pseudonymisation domain, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PseudonymSeed {
    /// The namespace.
    pub namespace: String,
    /// The name a steward reads.
    pub name: String,
    /// The client id a pseudonym is minted for in ITI-83.
    pub consumer_client_id: Option<String>,
}

/// Resolves the seed against the identifier domains `domains` it may not
/// share a system with.
///
/// # Errors
/// Returns [`Error::Missing`] for an entry without its namespace or name,
/// [`Error::Value`] for a namespace that is no absolute URI or a blank
/// consumer, and [`Error::DuplicateNamespace`] for a namespace seeded twice
/// or held by an identifier domain.
pub fn resolve(
    entries: &[PseudonymDomain],
    domains: &[DomainSeed],
) -> Result<Vec<PseudonymSeed>, Error> {
    let systems: BTreeSet<&str> = domains
        .iter()
        .map(|domain| domain.system.as_str())
        .collect();
    let mut seen = BTreeSet::new();
    let mut seeds = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let key = |field: &str| format!("pseudonym_domain[{index}].{field}");
        for (field, text) in [("namespace", &entry.namespace), ("name", &entry.name)] {
            if text.trim().is_empty() {
                return Err(Error::Missing { key: key(field) });
            }
        }
        if !is_system(&entry.namespace) {
            return Err(Error::Value {
                key: key("namespace"),
                rule: "an absolute URI without whitespace, and a urn:oid: that is a FHIR R4 oid",
            });
        }
        if entry
            .consumer_client_id
            .as_deref()
            .is_some_and(|client| client.trim().is_empty())
        {
            return Err(Error::Value {
                key: key("consumer_client_id"),
                rule: "a client id, or the key left out",
            });
        }
        if systems.contains(entry.namespace.as_str()) || !seen.insert(entry.namespace.as_str()) {
            return Err(Error::DuplicateNamespace {
                index,
                namespace: entry.namespace.clone(),
            });
        }
        seeds.push(PseudonymSeed {
            namespace: entry.namespace.clone(),
            name: entry.name.trim().to_owned(),
            consumer_client_id: entry
                .consumer_client_id
                .as_deref()
                .map(|client| client.trim().to_owned()),
        });
    }
    Ok(seeds)
}

#[cfg(test)]
mod tests {
    use super::{PseudonymDomain, resolve};
    use crate::config::error::Error;
    use crate::config::section::domain::{DomainKind, DomainSeed};

    fn entry(namespace: &str) -> PseudonymDomain {
        PseudonymDomain {
            namespace: namespace.to_owned(),
            name: String::from("Clinical domain"),
            consumer_client_id: Some(String::from("ferroehr")),
        }
    }

    #[test]
    fn a_namespace_is_a_uri_seeded_once_and_held_by_no_identifier_domain() {
        let domain = DomainSeed {
            system: String::from("urn:oid:2.999.1.1"),
            name: String::from("East"),
            kind: DomainKind::Local,
            source_client_id: String::from("east-lab"),
            sensitive: true,
        };
        let seeds = resolve(&[entry("urn:oid:2.999.9.1")], std::slice::from_ref(&domain))
            .expect("resolves");
        assert_eq!(Some("ferroehr"), seeds[0].consumer_client_id.as_deref());
        assert!(matches!(
            resolve(&[entry("not a uri")], &[]),
            Err(Error::Value { .. })
        ));
        assert!(matches!(
            resolve(
                &[entry("urn:oid:2.999.9.1"), entry("urn:oid:2.999.9.1")],
                &[]
            ),
            Err(Error::DuplicateNamespace { index: 1, .. })
        ));
        assert!(matches!(
            resolve(&[entry("urn:oid:2.999.1.1")], &[domain]),
            Err(Error::DuplicateNamespace { index: 0, .. })
        ));
    }
}
