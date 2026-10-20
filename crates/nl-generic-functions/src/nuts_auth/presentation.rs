// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Presentation Definition an authorization server asks for, and the
//! Presentation Submission that maps the holder's credentials to it
//! (Presentation Exchange 2.0.0; Nuts RFC021 §3, §5).
//!
//! The holder states which input descriptor each of its credentials answers
//! ([`Holder`]); this module checks that answer against the definition the
//! authorization server serves for the scope, before any credential is sent:
//! every credential names an input descriptor of the definition, and the
//! credentials together meet the definition's Submission Requirements, or,
//! when it has none, answer every input descriptor (Presentation Exchange
//! 2.0.0 §Submission Requirement Feature, §Input Evaluation). The
//! constraints of each descriptor are the authorization server's to evaluate
//! against the presented credentials (RFC021 §4.1); this client does not
//! evaluate them.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::nuts_auth::holder::Holder;

/// The `format` of a JWT-encoded Verifiable Credential in a descriptor map
/// (Presentation Exchange 2.0.0 §Claim Format Designations).
pub const JWT_VC: &str = "jwt_vc";

/// The `format` of a JWT-encoded Verifiable Presentation in authorization
/// server metadata (Presentation Exchange 2.0.0 §Claim Format Designations,
/// Nuts RFC021 §3.1).
pub const JWT_VP: &str = "jwt_vp";

/// A definition the holder's credentials do not answer.
///
/// Each variant names input descriptors and requirement positions, never a
/// credential.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Mismatch {
    /// A credential answers an input descriptor the definition does not
    /// have.
    #[error("the definition has no input descriptor {descriptor:?}")]
    UnknownDescriptor {
        /// The descriptor the credential names.
        descriptor: String,
    },
    /// The definition has no Submission Requirements, and an input
    /// descriptor is answered by no credential.
    #[error("no credential answers input descriptor {descriptor:?}")]
    Unanswered {
        /// The descriptor no credential answers.
        descriptor: String,
    },
    /// A Submission Requirement is not met by the credentials.
    #[error("submission requirement {index} is not met")]
    Requirement {
        /// The requirement's position in the definition.
        index: usize,
    },
    /// A Submission Requirement is neither `all` nor `pick`, or names
    /// neither `from` nor `from_nested`, or both (Presentation Exchange
    /// 2.0.0 §Submission Requirement Objects).
    #[error("submission requirement {index} is not a valid requirement")]
    InvalidRequirement {
        /// The requirement's position in the definition.
        index: usize,
    },
}

/// A Presentation Definition, the members this client reads (Presentation
/// Exchange 2.0.0 §Presentation Definition).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PresentationDefinition {
    /// The definition's `id`, the `definition_id` of the submission.
    pub id: String,
    /// The input descriptors.
    pub input_descriptors: Vec<InputDescriptor>,
    /// The Submission Requirements, when the definition has any.
    #[serde(default)]
    pub submission_requirements: Option<Vec<SubmissionRequirement>>,
}

/// An Input Descriptor, the members this client reads (Presentation Exchange
/// 2.0.0 §Input Descriptor Object).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InputDescriptor {
    /// The descriptor's `id`.
    pub id: String,
    /// The groups the descriptor belongs to, for the Submission Requirements.
    #[serde(default)]
    pub group: Vec<String>,
}

/// A Submission Requirement (Presentation Exchange 2.0.0 §Submission
/// Requirement Objects).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SubmissionRequirement {
    /// `all` or `pick`.
    pub rule: String,
    /// For `pick`, the exact number to submit.
    pub count: Option<u64>,
    /// For `pick`, the least number to submit.
    pub min: Option<u64>,
    /// For `pick`, the most number to submit.
    pub max: Option<u64>,
    /// The group of input descriptors the requirement is over.
    pub from: Option<String>,
    /// The requirements this one is over.
    pub from_nested: Option<Vec<SubmissionRequirement>>,
}

/// A Presentation Submission (Presentation Exchange 2.0.0 §Presentation
/// Submission).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PresentationSubmission {
    /// A fresh identifier of this submission.
    pub id: String,
    /// The `id` of the definition it answers.
    pub definition_id: String,
    /// Where each answered input descriptor's credential sits.
    pub descriptor_map: Vec<DescriptorMapping>,
}

/// One entry of a descriptor map (Presentation Exchange 2.0.0 §Presentation
/// Submission).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DescriptorMapping {
    /// The input descriptor answered.
    pub id: String,
    /// The credential's format, [`JWT_VC`].
    pub format: String,
    /// The `JSONPath` of the credential in the presentation.
    pub path: String,
}

/// The submission that maps every credential of `holder` to the input
/// descriptor of `definition` it answers, under the submission `id`.
///
/// The presentation carries every credential, in the holder's order, so
/// credential `i` sits at `$.verifiableCredential[i]`.
///
/// # Errors
///
/// Returns a [`Mismatch`] when a credential answers no descriptor of
/// `definition`, or the credentials do not meet its requirements.
pub fn submission(
    definition: &PresentationDefinition,
    holder: &Holder,
    id: String,
) -> Result<PresentationSubmission, Mismatch> {
    let known: BTreeSet<&str> = definition
        .input_descriptors
        .iter()
        .map(|descriptor| descriptor.id.as_str())
        .collect();
    let mut answered = BTreeSet::new();
    for credential in holder.credentials() {
        if !known.contains(credential.descriptor()) {
            return Err(Mismatch::UnknownDescriptor {
                descriptor: credential.descriptor().to_owned(),
            });
        }
        answered.insert(credential.descriptor());
    }
    match &definition.submission_requirements {
        None => {
            if let Some(missing) = definition
                .input_descriptors
                .iter()
                .find(|descriptor| !answered.contains(descriptor.id.as_str()))
            {
                return Err(Mismatch::Unanswered {
                    descriptor: missing.id.clone(),
                });
            }
        }
        Some(requirements) => {
            for (index, requirement) in requirements.iter().enumerate() {
                match met(requirement, definition, &answered) {
                    Some(true) => {}
                    Some(false) => return Err(Mismatch::Requirement { index }),
                    None => return Err(Mismatch::InvalidRequirement { index }),
                }
            }
        }
    }
    let descriptor_map = holder
        .credentials()
        .iter()
        .enumerate()
        .map(|(position, credential)| DescriptorMapping {
            id: credential.descriptor().to_owned(),
            format: JWT_VC.to_owned(),
            path: format!("$.verifiableCredential[{position}]"),
        })
        .collect();
    Ok(PresentationSubmission {
        id,
        definition_id: definition.id.clone(),
        descriptor_map,
    })
}

/// Whether `requirement` is met by the `answered` descriptors of
/// `definition`, or `None` when it is not a valid requirement.
fn met(
    requirement: &SubmissionRequirement,
    definition: &PresentationDefinition,
    answered: &BTreeSet<&str>,
) -> Option<bool> {
    let (total, satisfied) = match (&requirement.from, &requirement.from_nested) {
        (Some(group), None) => {
            let members: Vec<&str> = definition
                .input_descriptors
                .iter()
                .filter(|descriptor| descriptor.group.iter().any(|name| name == group))
                .map(|descriptor| descriptor.id.as_str())
                .collect();
            let satisfied = members.iter().filter(|id| answered.contains(*id)).count();
            (members.len(), satisfied)
        }
        (None, Some(nested)) => {
            let mut satisfied = 0_usize;
            for inner in nested {
                if met(inner, definition, answered)? {
                    satisfied = satisfied.saturating_add(1);
                }
            }
            (nested.len(), satisfied)
        }
        _ => return None,
    };
    let satisfied = u64::try_from(satisfied).ok()?;
    match requirement.rule.as_str() {
        "all" => Some(u64::try_from(total).ok()? == satisfied),
        "pick" => Some(
            requirement.count.is_none_or(|count| satisfied == count)
                && requirement.min.is_none_or(|min| satisfied >= min)
                && requirement.max.is_none_or(|max| satisfied <= max),
        ),
        _ => None,
    }
}
