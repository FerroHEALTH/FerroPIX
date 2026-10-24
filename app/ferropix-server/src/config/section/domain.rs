// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[[domain]]` seed: the identifier domains written to the store at
//! start when they are absent, never updated, so the database stays the
//! record.
//!
//! A domain is the `system` of a FHIR identifier (FHIR R4 `Identifier.system`,
//! a `uri`) fed by exactly one Patient Identity Source (PIXm 3.1.0
//! §2:3.104.4.1.3), which the token's client id names. No specification
//! governs the seed itself: our own design.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::config::error::Error;
use crate::config::section::auth::AuthSettings;

/// What a domain's identifiers are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DomainKind {
    /// A system's own numbers, never linked on across domains.
    Local,
    /// A national number the deterministic match may link on.
    Authoritative,
    /// FerroPIX's own domain, the PMIR Master Patient Identification Domain.
    Master,
}

/// One `[[domain]]` entry as written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Domain {
    /// The identifier system: `urn:oid:…`, or a URL.
    pub system: String,
    /// The name a steward reads.
    pub name: String,
    /// What the domain's identifiers are.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<DomainKind>,
    /// The client id of the domain's one Patient Identity Source.
    pub source_client_id: String,
    /// Whether the identifier values are sealed at rest.
    pub sensitive: bool,
}

impl Default for Domain {
    fn default() -> Self {
        Self {
            system: String::new(),
            name: String::new(),
            kind: None,
            source_client_id: String::new(),
            sensitive: true,
        }
    }
}

/// One seeded domain, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainSeed {
    /// The identifier system.
    pub system: String,
    /// The name a steward reads.
    pub name: String,
    /// What the domain's identifiers are.
    pub kind: DomainKind,
    /// The client id of the domain's one Patient Identity Source.
    pub source_client_id: String,
    /// Whether the identifier values are sealed at rest.
    pub sensitive: bool,
}

/// Resolves the seed against the resolved `[auth]` section.
///
/// # Errors
/// Returns [`Error::Missing`] for an entry without its system, name, kind or
/// client, [`Error::Value`] for a system that is no absolute URI or no FHIR
/// `oid`, [`Error::DuplicateSystem`] for a system seeded twice and
/// [`Error::ClientNotAdmitted`] for a client no issuer lists.
pub fn resolve(domains: &[Domain], auth: &AuthSettings) -> Result<Vec<DomainSeed>, Error> {
    let mut seen = BTreeSet::new();
    let mut seeds = Vec::with_capacity(domains.len());
    for (index, domain) in domains.iter().enumerate() {
        let key = |field: &str| format!("domain[{index}].{field}");
        for (field, text) in [
            ("system", &domain.system),
            ("name", &domain.name),
            ("source_client_id", &domain.source_client_id),
        ] {
            if text.trim().is_empty() {
                return Err(Error::Missing { key: key(field) });
            }
        }
        let kind = domain
            .kind
            .ok_or_else(|| Error::Missing { key: key("kind") })?;
        if !is_system(&domain.system) {
            return Err(Error::Value {
                key: key("system"),
                rule: "an absolute URI without whitespace, and a urn:oid: that is a FHIR R4 oid",
            });
        }
        if !seen.insert(domain.system.as_str()) {
            return Err(Error::DuplicateSystem {
                index,
                system: domain.system.clone(),
            });
        }
        if !auth.admits(&domain.source_client_id) {
            return Err(Error::ClientNotAdmitted {
                index,
                client: domain.source_client_id.clone(),
            });
        }
        seeds.push(DomainSeed {
            system: domain.system.clone(),
            name: domain.name.clone(),
            kind,
            source_client_id: domain.source_client_id.clone(),
            sensitive: domain.sensitive,
        });
    }
    Ok(seeds)
}

/// Returns whether `system` is an absolute URI with no whitespace, and, when
/// it starts with `urn:oid:`, the FHIR R4 `oid` type,
/// `urn:oid:[0-2](\.(0|[1-9][0-9]*))+` (<https://hl7.org/fhir/R4/datatypes.html#oid>).
fn is_system(system: &str) -> bool {
    if system.chars().any(char::is_whitespace) || url::Url::parse(system).is_err() {
        return false;
    }
    let Some(oid) = system.strip_prefix("urn:oid:") else {
        return true;
    };
    let mut arcs = oid.split('.');
    let first_ok = matches!(arcs.next(), Some("0" | "1" | "2"));
    let rest: Vec<&str> = arcs.collect();
    first_ok
        && !rest.is_empty()
        && rest.iter().all(|arc| {
            !arc.is_empty()
                && arc.chars().all(|c| c.is_ascii_digit())
                && (*arc == "0" || !arc.starts_with('0'))
        })
}

#[cfg(test)]
mod tests {
    use super::is_system;

    #[test]
    fn a_system_is_an_absolute_uri_and_an_oid_follows_the_fhir_pattern() {
        for good in [
            "urn:oid:2.16.840.1.113883.2.4.6.3",
            "urn:oid:1.0",
            "https://example.org/fhir/sid/local",
            "urn:uuid:0f0e7b8a-31c2-4d3f-9a43-1f5f0a3a6e11",
        ] {
            assert!(is_system(good), "{good}");
        }
        for bad in [
            "",
            "local",
            "urn:oid:3.1",
            "urn:oid:2",
            "urn:oid:2.01",
            "urn:oid:2..1",
            "urn:oid:2.a",
            "https://example.org/a b",
        ] {
            assert!(!is_system(bad), "{bad}");
        }
    }
}
