// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The identifiers of ITI-55: ISO object identifiers, the patient identifier
//! a discovery asks about, a home community, and the identifiers a
//! responding community answers with.

use std::fmt;

use secrecy::SecretString;

use super::error::InvalidInput;
use crate::redact::REDACTED;

/// The URN prefix of an ISO object identifier (RFC 3061).
const URN_OID: &str = "urn:oid:";

/// An ISO object identifier in dotted form: the `root` of an HL7 v3 `II`
/// that names a device, an assigning authority or a home community (ITI TF-2
/// Appendix E; Appendix O: "id.root SHALL be an ISO OID").
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Oid(String);

impl Oid {
    /// Reads `text`, in dotted form (`2.999.1`) or as a URN
    /// (`urn:oid:2.999.1`, RFC 3061).
    ///
    /// # Errors
    /// [`InvalidInput::Oid`] when `text` is not two or more arcs of decimal
    /// digits, the first `0`, `1` or `2`, with no arc but `0` itself
    /// starting with `0`.
    pub fn new(text: &str) -> Result<Self, InvalidInput> {
        let dotted = text.strip_prefix(URN_OID).unwrap_or(text);
        let mut arcs = dotted.split('.');
        let first = arcs.next().unwrap_or_default();
        if !matches!(first, "0" | "1" | "2") {
            return Err(InvalidInput::Oid);
        }
        let mut count = 1_usize;
        for arc in arcs {
            let digits = !arc.is_empty() && arc.bytes().all(|byte| byte.is_ascii_digit());
            if !digits || (arc.len() > 1 && arc.starts_with('0')) {
                return Err(InvalidInput::Oid);
            }
            count = count.saturating_add(1);
        }
        if count < 2 {
            return Err(InvalidInput::Oid);
        }
        Ok(Self(dotted.to_owned()))
    }

    /// The dotted form, as an `II` `root` carries it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The URN form, `urn:oid:` and the dotted form (RFC 3061).
    #[must_use]
    pub fn urn(&self) -> String {
        format!("{URN_OID}{}", self.0)
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The globally unique identifier of a community (ITI TF-2 §3.38.4.1.2.1),
/// an OID whose URN form is the `homeCommunityId`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HomeCommunityId(Oid);

impl HomeCommunityId {
    /// The community named by `oid`.
    #[must_use]
    pub fn new(oid: Oid) -> Self {
        Self(oid)
    }

    /// The community's OID, which an `id` element carries as its only
    /// `root` (§3.55.4.1.2.4, §3.55.4.2.2.4).
    #[must_use]
    pub fn oid(&self) -> &Oid {
        &self.0
    }
}

impl fmt::Display for HomeCommunityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.urn())
    }
}

/// The patient identifier a discovery asks about.
///
/// It is the shared or national identifier of the third request mode
/// (§3.55.1), sent as the `LivingSubjectId` `II` with its assigning
/// authority as `root` and the value as `extension` (§3.55.4.1.2.1).
///
/// It is a directly identifying value: `Debug` shows the assigning
/// authority and redacts the value, and there is no `Display`.
#[derive(Clone)]
pub struct PatientIdentifier {
    authority: Oid,
    value: SecretString,
}

impl PatientIdentifier {
    /// The identifier `value` issued by `authority`.
    ///
    /// # Errors
    /// [`InvalidInput::EmptyValue`] when `value` is empty.
    pub fn new(authority: Oid, value: SecretString) -> Result<Self, InvalidInput> {
        use secrecy::ExposeSecret as _;
        if value.expose_secret().is_empty() {
            return Err(InvalidInput::EmptyValue);
        }
        Ok(Self { authority, value })
    }

    /// The assigning authority.
    #[must_use]
    pub fn authority(&self) -> &Oid {
        &self.authority
    }

    /// The identifier value.
    #[must_use]
    pub fn value(&self) -> &SecretString {
        &self.value
    }
}

impl fmt::Debug for PatientIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PatientIdentifier")
            .field("authority", &self.authority)
            .field("value", &REDACTED)
            .finish()
    }
}

/// One identifier a responding community holds for the matched patient: an
/// `II` of the `Patient` role (§3.55.4.2.2.2).
///
/// The `root` names an assigning authority and the `extension` the patient
/// within it; an `II` with no `extension` is identified by its `root` alone.
/// `Debug` redacts whichever of the two identifies the patient.
#[derive(Clone)]
pub struct CommunityPatientId {
    root: SecretString,
    extension: Option<SecretString>,
}

impl CommunityPatientId {
    pub(super) fn new(root: SecretString, extension: Option<SecretString>) -> Self {
        Self { root, extension }
    }

    /// The `root`: the assigning authority, or the identifier itself when
    /// there is no `extension` (HL7 v3 `II`).
    #[must_use]
    pub fn root(&self) -> &SecretString {
        &self.root
    }

    /// The `extension`, the identifier within the `root`'s scope.
    #[must_use]
    pub fn extension(&self) -> Option<&SecretString> {
        self.extension.as_ref()
    }
}

impl fmt::Debug for CommunityPatientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use secrecy::ExposeSecret as _;
        let root = match self.extension {
            Some(_) => self.root.expose_secret(),
            None => REDACTED,
        };
        f.debug_struct("CommunityPatientId")
            .field("root", &root)
            .field("extension", &self.extension.as_ref().map(|_| REDACTED))
            .finish()
    }
}
