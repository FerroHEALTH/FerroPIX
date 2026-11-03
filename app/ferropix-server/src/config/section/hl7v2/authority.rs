// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The identifier domain an HL7 v2 assigning authority names, and the
//! assigning authority a domain is written with.
//!
//! An assigning authority is the `HD` in CX-4: a namespace id, or a universal
//! id with its type, or all three, which then name the same entity (ITI TF-2
//! §3.8.4.1.2.3, §3.9.4.1.2.2). A domain is the `system` of a FHIR
//! identifier. A universal id of type `ISO` is the OID of the system
//! `urn:oid:<oid>` and one of type `URI` is the system itself; a namespace
//! id is found in `[hl7v2.namespaces]`. An answer writes every subcomponent
//! (ITI TF-2 §3.9.4.2.2.5: "the Assigning Authority component returned shall
//! include ALL subcomponents"), the namespace id empty for a domain the
//! configuration names none for. No specification governs the mapping from
//! an `HD` to a FHIR system: our own design.

use std::collections::BTreeMap;

use ferrobridge_hl7v2::parse::Component;

/// The prefix of a system that names an OID.
const OID: &str = "urn:oid:";

/// One assigning authority as a message carries it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hd {
    /// HD-1, the namespace id.
    pub namespace: Option<String>,
    /// HD-2, the universal id.
    pub universal: Option<String>,
    /// HD-3, the universal id's type.
    pub kind: Option<String>,
}

impl Hd {
    /// Reads the assigning authority from the subcomponents of `component`.
    #[must_use]
    pub fn of(component: Option<&Component>) -> Self {
        let part = |position: usize| {
            component
                .and_then(|component| component.subcomponent(position))
                .map(str::to_owned)
        };
        Self {
            namespace: part(1),
            universal: part(2),
            kind: part(3),
        }
    }

    /// Returns whether no subcomponent is valued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.namespace.is_none() && self.universal.is_none() && self.kind.is_none()
    }
}

/// The namespace ids of the domains, both ways.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Authorities {
    /// The system each namespace id names.
    by_namespace: BTreeMap<String, String>,
    /// The namespace id of each system.
    by_system: BTreeMap<String, String>,
}

impl Authorities {
    /// Returns the authorities of `namespaces`, the namespace id of each
    /// system, which the configuration admitted one per namespace.
    #[must_use]
    pub fn new(namespaces: &BTreeMap<String, String>) -> Self {
        Self {
            by_namespace: namespaces
                .iter()
                .map(|(system, namespace)| (namespace.clone(), system.clone()))
                .collect(),
            by_system: namespaces.clone(),
        }
    }

    /// Returns the system `hd` names, `None` for one it names no domain by.
    #[must_use]
    pub fn system(&self, hd: &Hd) -> Option<String> {
        if let (Some(universal), Some(kind)) = (&hd.universal, &hd.kind) {
            if kind.eq_ignore_ascii_case("ISO") {
                return Some(format!("{OID}{universal}"));
            }
            if kind.eq_ignore_ascii_case("URI") {
                return Some(universal.clone());
            }
        }
        hd.namespace
            .as_ref()
            .and_then(|namespace| self.by_namespace.get(namespace))
            .cloned()
    }

    /// Returns the assigning authority of `system`: its namespace id, its
    /// universal id and the universal id's type.
    #[must_use]
    pub fn of_system(&self, system: &str) -> [String; 3] {
        let namespace = self.by_system.get(system).cloned().unwrap_or_default();
        match system.strip_prefix(OID) {
            Some(oid) => [namespace, oid.to_owned(), String::from("ISO")],
            None => [namespace, system.to_owned(), String::from("URI")],
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Authorities, Hd};

    fn authorities() -> Authorities {
        Authorities::new(&BTreeMap::from([(
            String::from("urn:oid:2.999.1.1"),
            String::from("EAST"),
        )]))
    }

    fn hd(namespace: Option<&str>, universal: Option<&str>, kind: Option<&str>) -> Hd {
        Hd {
            namespace: namespace.map(str::to_owned),
            universal: universal.map(str::to_owned),
            kind: kind.map(str::to_owned),
        }
    }

    #[test]
    fn every_form_of_an_assigning_authority_names_its_domain() {
        let authorities = authorities();
        let east = Some(String::from("urn:oid:2.999.1.1"));
        assert_eq!(authorities.system(&hd(Some("EAST"), None, None)), east);
        assert_eq!(
            authorities.system(&hd(None, Some("2.999.1.1"), Some("ISO"))),
            east
        );
        assert_eq!(
            authorities.system(&hd(Some("EAST"), Some("2.999.1.1"), Some("ISO"))),
            east
        );
        assert_eq!(
            authorities.system(&hd(None, Some("https://example.org/mrn"), Some("URI"))),
            Some(String::from("https://example.org/mrn"))
        );
        assert_eq!(authorities.system(&hd(Some("WEST"), None, None)), None);
        assert_eq!(
            authorities.system(&hd(None, Some("example.org"), Some("DNS"))),
            None
        );
    }

    #[test]
    fn an_answer_writes_every_subcomponent() {
        let authorities = authorities();
        assert_eq!(
            authorities.of_system("urn:oid:2.999.1.1"),
            [
                String::from("EAST"),
                String::from("2.999.1.1"),
                String::from("ISO")
            ]
        );
        assert_eq!(
            authorities.of_system("https://ferropix.example/"),
            [
                String::new(),
                String::from("https://ferropix.example/"),
                String::from("URI")
            ]
        );
    }
}
