// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Patient of an ITI-104 message, as the Source writes it and the
//! Manager reads it (§2:3.104.4.1.2, §2:3.104.4.2.2).

use fhir_types::r4::identifier::Identifier;
use fhir_types::r4::patient::{Patient, PatientLink};
use secrecy::ExposeSecret;

use super::identifier::SourceIdentifier;

/// The `link.type` of a Resolve Duplicate Patient message (§2:3.104.4.2.2).
pub(super) const REPLACED_BY: &str = "replaced-by";

/// Whether `patient` holds to the constraints of the PDQm Patient profile: no
/// `implicitRules` and no `modifierExtension`, and at least one identifier,
/// each with a `system` and a `value` (§2:3.104.4.1.2.1).
pub(super) fn holds_to_profile(patient: &Patient) -> bool {
    let present = |text: Option<&str>| text.is_some_and(|text| !text.is_empty());
    let complete = |identifier: &Identifier| {
        present(identifier.system.as_ref().and_then(|s| s.value.as_deref()))
            && present(identifier.value.as_ref().and_then(|v| v.value.as_deref()))
    };
    patient.implicit_rules.is_none()
        && patient.modifier_extension.is_empty()
        && !patient.identifier.is_empty()
        && patient.identifier.iter().all(complete)
}

/// Whether `patient` carries `identifier`.
pub(super) fn carries(patient: &Patient, identifier: &SourceIdentifier) -> bool {
    patient.identifier.iter().any(|carried| {
        carried.system.as_ref().and_then(|s| s.value.as_deref()) == Some(identifier.system())
            && carried.value.as_ref().and_then(|v| v.value.as_deref())
                == Some(identifier.value().expose_secret())
    })
}

/// The `replaced-by` links of `patient`.
pub(super) fn replaced_by(patient: &Patient) -> impl Iterator<Item = &PatientLink> {
    patient
        .link
        .iter()
        .filter(|link| link.r#type.value.as_deref() == Some(REPLACED_BY))
}
