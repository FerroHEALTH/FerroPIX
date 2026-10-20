// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What a responding gateway answered: the patient's matches, no match, or a
//! request for more demographics (§3.55.4.2.3, Cases 1 to 4).

use std::collections::BTreeSet;

use super::identifier::{CommunityPatientId, HomeCommunityId};

/// The answer of one responding gateway to a discovery.
///
/// Case 5 of §3.55.4.2.3, a gateway that cannot satisfy the request, is an
/// [`XcpdError`](super::error::XcpdError), never one of these.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Discovery {
    /// Cases 1 and 2: one or more matching patient records, one per
    /// registration event, each with the community that holds it.
    Matched(Vec<Match>),
    /// Case 4: no patient anywhere close to matching; the initiating
    /// gateway "can assume this patient has no healthcare information held
    /// by the community".
    NoMatch,
    /// Case 3: candidates that do not match closely enough, and the
    /// demographics that might let the gateway return one
    /// (§3.55.4.2.2.6).
    MoreAttributesRequested(Vec<RequestedAttribute>),
}

impl Discovery {
    /// Every community a match names, once each: the sets of registration
    /// events §3.55.4.2.2.4 asks the initiating gateway to read by
    /// `homeCommunityId`.
    #[must_use]
    pub fn communities(&self) -> BTreeSet<&HomeCommunityId> {
        match self {
            Self::Matched(found) => found.iter().map(Match::community).collect(),
            Self::NoMatch | Self::MoreAttributesRequested(_) => BTreeSet::new(),
        }
    }
}

/// One matching patient record: one registration event of the response
/// (§3.55.4.2.2.2, §3.55.4.2.2.4).
#[derive(Debug, Clone)]
pub struct Match {
    community: HomeCommunityId,
    patient_ids: Vec<CommunityPatientId>,
}

impl Match {
    pub(super) fn new(community: HomeCommunityId, patient_ids: Vec<CommunityPatientId>) -> Self {
        Self {
            community,
            patient_ids,
        }
    }

    /// The community that holds the record: the `homeCommunityId` of the
    /// event's custodian.
    #[must_use]
    pub fn community(&self) -> &HomeCommunityId {
        &self.community
    }

    /// The patient's identifiers in that community, the first the one to use
    /// in a later Cross Gateway Query to it (Table 3.55.4.2.2.2-1,
    /// `Patient.id`).
    #[must_use]
    pub fn patient_ids(&self) -> &[CommunityPatientId] {
        &self.patient_ids
    }
}

/// A demographic attribute a responding gateway asks for, from code system
/// `1.3.6.1.4.1.19376.1.2.27.1` (Table 3.55.4.2.2.6-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RequestedAttribute {
    /// `LivingSubjectAdministrativeGenderRequested`.
    AdministrativeGender,
    /// `PatientAddressRequested`.
    PatientAddress,
    /// `PatientTelecomRequested`.
    PatientTelecom,
    /// `LivingSubjectBirthPlaceNameRequested`.
    BirthPlaceName,
    /// `LivingSubjectBirthPlaceAddressRequested`.
    BirthPlaceAddress,
    /// `MothersMaidenNameRequested`.
    MothersMaidenName,
    /// A code the table does not define.
    Other,
}

impl RequestedAttribute {
    pub(super) fn of(code: &str) -> Self {
        match code {
            "LivingSubjectAdministrativeGenderRequested" => Self::AdministrativeGender,
            "PatientAddressRequested" => Self::PatientAddress,
            "PatientTelecomRequested" => Self::PatientTelecom,
            "LivingSubjectBirthPlaceNameRequested" => Self::BirthPlaceName,
            "LivingSubjectBirthPlaceAddressRequested" => Self::BirthPlaceAddress,
            "MothersMaidenNameRequested" => Self::MothersMaidenName,
            _ => Self::Other,
        }
    }
}
