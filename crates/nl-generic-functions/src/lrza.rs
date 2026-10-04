// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! GF-Addressing through the national address book, LRZa (feature `lrza`).
//!
//! The IG's Care Services Directory follows IHE mCSD, with the LRZa as the
//! authoritative source of every top-level care provider `Organization`: its
//! URA identifier and its name (the IG's Care Services Directory page, Update
//! Client). An `Organization` either carries a URA identifier, a top-level
//! care provider, or is `partOf` another `Organization` (the
//! `ura-identifier-or-partof` invariant of the `nl-gf-organization` and
//! `nl-gf-organization-lrza` profiles).
//!
//! [`ura`] reads that identifier, so a directory reader can join a
//! GF-Localization custodian, named by URA, to the `Organization` and the
//! `Endpoint`s the directory publishes for it. The directory itself is read
//! over mCSD ITI-90 and ITI-91, which this crate does not repeat.
//!
//! An `Endpoint` of the GF directory carries `connectionType`
//! `hl7-fhir-rest` in its examples, which does not denote an openEHR Query
//! API; whoever reads openEHR endpoints from it applies its own
//! `connectionType` rule (Federation Tier §15.2, N19, CP-20, Annex B §B.2).
//!
//! # Examples
//!
//! ```
//! use fhir_types::r4::identifier::Identifier;
//! use fhir_types::r4::organization::Organization;
//! use fhir_types::r4::primitives::{String as FhirString, Uri};
//! use nl_generic_functions::identification::URA_SYSTEM;
//! use nl_generic_functions::lrza;
//!
//! let organization = Organization {
//!     identifier: vec![Identifier {
//!         system: Some(Uri { value: Some(URA_SYSTEM.to_owned()), ..Uri::default() }),
//!         value: Some(FhirString { value: Some("ura-test-0001".to_owned()), ..FhirString::default() }),
//!         ..Identifier::default()
//!     }],
//!     ..Organization::default()
//! };
//! let ura = lrza::ura(&organization)?;
//! assert_eq!(ura.map(|ura| ura.to_string()).as_deref(), Some("ura-test-0001"));
//! # Ok::<(), lrza::LrzaError>(())
//! ```

use std::collections::BTreeSet;

use fhir_types::r4::organization::Organization;

use crate::identification::{URA_SYSTEM, Ura};

/// Why an `Organization` has no URA to give.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LrzaError {
    /// The `Organization` carries no URA identifier and is `partOf` no other
    /// `Organization`, against the `ura-identifier-or-partof` invariant.
    #[error("the Organization carries no URA identifier and is part of no other Organization")]
    Missing,
    /// A URA identifier carries no value.
    #[error("a URA identifier of the Organization carries no value")]
    EmptyValue,
    /// The `Organization` carries two different URAs, so it names no one
    /// care provider (no specification governs this: our own design).
    #[error("the Organization carries more than one URA")]
    Ambiguous,
}

/// Returns the URA of `organization`: `Some` for a top-level care provider,
/// `None` for an `Organization` that is `partOf` another.
///
/// # Errors
///
/// [`LrzaError::Missing`] for an `Organization` that is neither,
/// [`LrzaError::EmptyValue`] for a URA identifier without a value, and
/// [`LrzaError::Ambiguous`] for two different URAs.
pub fn ura(organization: &Organization) -> Result<Option<Ura>, LrzaError> {
    let mut found = BTreeSet::new();
    for identifier in &organization.identifier {
        let system = identifier
            .system
            .as_ref()
            .and_then(|uri| uri.value.as_deref());
        if system != Some(URA_SYSTEM) {
            continue;
        }
        let value = identifier
            .value
            .as_ref()
            .and_then(|value| value.value.as_deref())
            .ok_or(LrzaError::EmptyValue)?;
        found.insert(Ura::new(value).map_err(|_empty| LrzaError::EmptyValue)?);
    }
    let mut found = found.into_iter();
    match (found.next(), found.next()) {
        (Some(ura), None) => Ok(Some(ura)),
        (Some(_), Some(_)) => Err(LrzaError::Ambiguous),
        (None, _) if organization.part_of.is_some() => Ok(None),
        (None, _) => Err(LrzaError::Missing),
    }
}
