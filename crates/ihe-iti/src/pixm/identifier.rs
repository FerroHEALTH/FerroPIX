// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The identifiers ITI-83 carries, and what a successful answer holds.
//!
//! Every identifier value is a [`SecretString`]: its `Debug` is redacted and it
//! has no `Display`, so a value reaches text only through a deliberate
//! [`ExposeSecret::expose_secret`] call (secrecy's own contract). The assigning
//! authorities and the target systems are domain names, not patient data, and
//! print as written.

use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::error::InvalidInput;
use crate::redact::REDACTED;

/// The patient identifier the Consumer asks about: the Patient Identifier
/// Domain (assigning authority) and the identifier value of the
/// `sourceIdentifier` parameter (§2:3.83.4.1.2.1).
#[derive(Clone)]
pub struct SourceIdentifier {
    system: String,
    value: SecretString,
}

impl SourceIdentifier {
    /// Creates the identifier `value` in the domain whose assigning authority
    /// is `system`, an absolute URI such as `urn:oid:2.999.1`.
    ///
    /// # Errors
    /// [`InvalidInput::System`] when `system` is not an absolute URI, and
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn new(system: impl Into<String>, value: SecretString) -> Result<Self, InvalidInput> {
        let system = absolute_uri(system.into())?;
        if value.expose_secret().is_empty() {
            return Err(InvalidInput::EmptyValue);
        }
        Ok(Self { system, value })
    }

    /// Returns the assigning authority of the identifier's domain.
    #[must_use]
    pub fn system(&self) -> &str {
        &self.system
    }

    /// Returns the identifier value.
    #[must_use]
    pub fn value(&self) -> &SecretString {
        &self.value
    }
}

impl fmt::Debug for SourceIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceIdentifier")
            .field("system", &self.system)
            .field("value", &REDACTED)
            .finish()
    }
}

/// A Patient Identifier Domain the answer's identifiers are selected from:
/// one `targetSystem` parameter (§2:3.83.4.1.2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSystem(String);

impl TargetSystem {
    /// Creates the domain whose assigning authority is `system`, an absolute
    /// URI.
    ///
    /// # Errors
    /// [`InvalidInput::System`] when `system` is not an absolute URI.
    pub fn new(system: impl Into<String>) -> Result<Self, InvalidInput> {
        Ok(Self(absolute_uri(system.into())?))
    }

    /// Returns the assigning authority, as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One `targetIdentifier` of the answer: an identifier another domain holds
/// for the patient, with its assigning authority (§2:3.83.4.2.2.1, ITI TF-2
/// Appendix E.3).
#[derive(Clone)]
pub struct TargetIdentifier {
    system: String,
    value: SecretString,
}

impl TargetIdentifier {
    pub(super) fn new(system: String, value: SecretString) -> Self {
        Self { system, value }
    }

    /// Returns the assigning authority of the domain the identifier belongs to.
    #[must_use]
    pub fn system(&self) -> &str {
        &self.system
    }

    /// Returns the identifier value.
    #[must_use]
    pub fn value(&self) -> &SecretString {
        &self.value
    }
}

impl fmt::Debug for TargetIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TargetIdentifier")
            .field("system", &self.system)
            .field("value", &REDACTED)
            .finish()
    }
}

/// One `targetId` of the answer: the reference of a matching Patient resource
/// (§2:3.83.4.2.2.1).
///
/// A logical id can be built from a business identifier, so the reference is
/// redacted like one.
#[derive(Clone)]
pub struct PatientReference(SecretString);

impl PatientReference {
    pub(super) fn new(reference: SecretString) -> Self {
        Self(reference)
    }

    /// Returns the reference, as the PIX Manager wrote it.
    #[must_use]
    pub fn reference(&self) -> &SecretString {
        &self.0
    }
}

impl fmt::Debug for PatientReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PatientReference({REDACTED})")
    }
}

/// The cross-referenced identifiers of a successful answer, in the PIX
/// Manager's order, which carries no meaning (§2:3.83.4.2.2.1).
#[derive(Debug, Clone, Default)]
pub struct CrossReferences {
    identifiers: Vec<TargetIdentifier>,
    patients: Vec<PatientReference>,
}

impl CrossReferences {
    pub(super) fn new(identifiers: Vec<TargetIdentifier>, patients: Vec<PatientReference>) -> Self {
        Self {
            identifiers,
            patients,
        }
    }

    /// Returns every `targetIdentifier`, possibly none: a patient the Manager
    /// knows may have no identifier in the domains asked about.
    #[must_use]
    pub fn identifiers(&self) -> &[TargetIdentifier] {
        &self.identifiers
    }

    /// Returns every `targetId`.
    #[must_use]
    pub fn patients(&self) -> &[PatientReference] {
        &self.patients
    }

    /// Returns the identifiers the domain `system` holds for the patient.
    pub fn in_domain<'a>(&'a self, system: &'a str) -> impl Iterator<Item = &'a TargetIdentifier> {
        self.identifiers
            .iter()
            .filter(move |identifier| identifier.system == system)
    }
}

/// What the PIX Manager answered about the patient.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum CrossReference {
    /// The Manager knows the patient, and these are the identifiers the asked
    /// domains hold (§2:3.83.4.2.2.1).
    Matched(CrossReferences),
    /// The Manager does not know the source identifier: a `404` whose
    /// `OperationOutcome` carries a `not-found` issue (§2:3.83.4.2.2.2), or a
    /// `200` with a Bundle holding no Patient after a merge or a delete
    /// (§2:3.83.4.2.2.5).
    SourceNotFound,
}

/// `text`, once it parses as an absolute URI.
fn absolute_uri(text: String) -> Result<String, InvalidInput> {
    if text.is_empty() || text.chars().any(char::is_whitespace) || Url::parse(&text).is_err() {
        return Err(InvalidInput::System);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::{
        CrossReferences, InvalidInput, PatientReference, SourceIdentifier, TargetIdentifier,
        TargetSystem,
    };
    use crate::redact::REDACTED;

    const SENTINEL: &str = "SENTINEL-4711";

    #[test]
    fn debug_never_shows_an_identifier_value() {
        let source = SourceIdentifier::new("urn:oid:2.999.1", SecretString::from(SENTINEL))
            .expect("a valid source identifier");
        let found = CrossReferences::new(
            vec![TargetIdentifier::new(
                "urn:oid:2.999.2".into(),
                SecretString::from(SENTINEL),
            )],
            vec![PatientReference::new(SecretString::from(format!(
                "Patient/{SENTINEL}"
            )))],
        );
        for shown in [format!("{source:?}"), format!("{found:?}")] {
            assert!(
                !shown.contains(SENTINEL),
                "Debug showed an identifier value"
            );
            assert!(shown.contains(REDACTED), "the placeholder shows: {shown}");
        }
        assert!(
            format!("{found:?}").contains("PatientReference(***)"),
            "a reference shows the family's placeholder"
        );
        assert!(
            format!("{source:?}").contains("urn:oid:2.999.1"),
            "the assigning authority is no patient data and prints"
        );
    }

    #[test]
    fn a_system_must_be_an_absolute_uri() {
        for bad in ["", "not a uri", "relative/path", "2.999.1"] {
            assert_eq!(
                SourceIdentifier::new(bad, SecretString::from("1")).err(),
                Some(InvalidInput::System),
                "{bad:?} is no assigning authority"
            );
            assert_eq!(
                TargetSystem::new(bad).err(),
                Some(InvalidInput::System),
                "{bad:?} is no target system"
            );
        }
        assert!(
            TargetSystem::new("urn:oid:2.999.1").is_ok(),
            "an OID URN is an absolute URI"
        );
    }

    #[test]
    fn an_empty_value_is_refused() {
        assert_eq!(
            SourceIdentifier::new("urn:oid:2.999.1", SecretString::from("")).err(),
            Some(InvalidInput::EmptyValue),
            "an empty identifier names no patient"
        );
    }
}
