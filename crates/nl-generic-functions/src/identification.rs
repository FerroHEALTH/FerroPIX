// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! GF-Identification: the identifier systems the other functions name a
//! patient and a care provider in (the IG's Identification page,
//! `input/pagecontent/identification.md`, and its naming systems).
//!
//! A patient is named by a pseudonymised BSN ([`PSEUDO_BSN_SYSTEM`]), never
//! by the BSN itself, and a care provider by its URA ([`URA_SYSTEM`]). A
//! [`PseudoBsn`] is personal data: its value is a [`SecretString`], its
//! `Debug` is redacted and it has no `Display`. A [`Ura`] names an
//! organisation, not a person, and prints as written.

use std::fmt;

#[cfg(feature = "nvi")]
use secrecy::{ExposeSecret, SecretString};

/// The naming system of a pseudonymised BSN, the patient identifier of a
/// localization record (the `pseudo-bsn` `NamingSystem` of the IG).
pub const PSEUDO_BSN_SYSTEM: &str = "http://fhir.nl/fhir/NamingSystem/pseudo-bsn";

/// The naming system of a URA, the identifier of a care provider in the
/// CIBG URA register (the IG's `$ura` alias).
pub const URA_SYSTEM: &str = "http://fhir.nl/fhir/NamingSystem/ura";

/// Why an identifier is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IdentifierError {
    /// The identifier value is empty.
    #[error("the identifier value is empty")]
    Empty,
}

/// A care provider's URA, the value of a [`URA_SYSTEM`] identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ura(String);

impl Ura {
    /// Creates the URA `value`.
    ///
    /// The IG fixes the system and leaves the value's form to the URA
    /// register, so any non-empty value is taken as written.
    ///
    /// # Errors
    ///
    /// [`IdentifierError::Empty`] when `value` is empty.
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        if value.is_empty() {
            return Err(IdentifierError::Empty);
        }
        Ok(Self(value))
    }

    /// Returns the URA as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Ura {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A pseudonymised BSN, the value of a [`PSEUDO_BSN_SYSTEM`] identifier.
///
/// The pseudonym is personal data like the BSN it stands for, so the value
/// reaches text only through a deliberate [`ExposeSecret::expose_secret`].
#[cfg(feature = "nvi")]
#[derive(Clone)]
pub struct PseudoBsn(SecretString);

#[cfg(feature = "nvi")]
impl PseudoBsn {
    /// Creates the pseudonymised BSN `value`.
    ///
    /// # Errors
    ///
    /// [`IdentifierError::Empty`] when `value` is empty.
    pub fn new(value: SecretString) -> Result<Self, IdentifierError> {
        if value.expose_secret().is_empty() {
            return Err(IdentifierError::Empty);
        }
        Ok(Self(value))
    }

    /// Returns the pseudonym.
    #[must_use]
    pub fn value(&self) -> &SecretString {
        &self.0
    }
}

#[cfg(feature = "nvi")]
impl fmt::Debug for PseudoBsn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PseudoBsn(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::{IdentifierError, Ura};

    #[test]
    fn an_empty_ura_is_refused_and_any_other_is_kept_as_written() {
        assert_eq!(Ura::new(""), Err(IdentifierError::Empty));
        let ura = Ura::new("ura-test-0001").expect("a URA");
        assert_eq!(ura.as_str(), "ura-test-0001");
        assert_eq!(ura.to_string(), "ura-test-0001");
    }

    #[cfg(feature = "nvi")]
    #[test]
    fn a_pseudonym_is_never_rendered() {
        use secrecy::SecretString;

        use super::PseudoBsn;

        assert!(PseudoBsn::new(SecretString::from("")).is_err());
        let pseudonym = PseudoBsn::new(SecretString::from("SENTINEL-pbsn")).expect("a pseudonym");
        assert!(!format!("{pseudonym:?}").contains("SENTINEL"));
    }
}
