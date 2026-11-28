// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why an ITI-104 message was not applied.
//!
//! No error here carries a request URL or body, an identifier value, a
//! demographic, or text the Manager wrote: the URL holds the identifier, the
//! body the Patient, and the Manager's `OperationOutcome` may quote either.

use crate::outcome::IssueType;

/// A message the Source refuses to send, because ITI-104 does not admit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidFeed {
    /// The Patient breaks a constraint of the PDQm Patient profile
    /// (§2:3.104.4.1.2.1).
    #[error("the Patient does not hold to the PDQm Patient profile")]
    Profile,
    /// The Patient does not carry the identifier the message names, which
    /// the Source provides in its own domain (§2:3.104.4.1.2).
    #[error("the Patient does not carry the identifier the message names")]
    IdentifierNotCarried,
    /// The Patient carries a `replaced-by` link of its own: an Add or Revise
    /// Patient message carries none, and a Resolve Duplicate Patient message
    /// carries the one link the client writes (§2:3.104.4.2.2).
    #[error("the Patient carries a replaced-by link of its own")]
    ReplacedBy,
    /// The surviving identifier is in another domain than the subsumed one:
    /// a Source merges within its own domain (§2:3.104.4.2.1).
    #[error("the surviving identifier is in another domain")]
    SurvivorInAnotherDomain,
    /// The surviving identifier is the subsumed one.
    #[error("the surviving identifier is the subsumed one")]
    SurvivorIsSubsumed,
}

manager_error! {
/// Why an ITI-104 message did not end in the Manager applying it.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SourceError {
    /// The Manager answered a status that does not apply the message, with
    /// the issue types its `OperationOutcome` carried, if it sent one.
    Rejected;
    /// The message was refused before anything was sent.
    #[error("the ITI-104 message is not admitted")]
    Invalid(#[from] InvalidFeed),
    /// The Patient could not be written, so nothing was sent.
    #[error("the ITI-104 message could not be written")]
    Unwritable(#[source] serde_json::Error),
    /// The identifier matched more than one resource at the Manager: a `412`
    /// (FHIR R4 http.html §3.1.0.4.3, §3.1.0.7.1).
    #[error("the identifier matched more than one resource at the PIX Manager")]
    NotSelective {
        /// The issue types of the `OperationOutcome`, in order.
        issues: Vec<IssueType>,
    },
    /// The audit recorder could not accept the exchange's audit record, so
    /// its outcome is not used (feature `balp`, §2:3.104.5.1.1).
    #[cfg(feature = "balp")]
    #[error("the ITI-104 audit record could not be recorded")]
    Audit(#[source] ihe_atna::balp::AuditError),
}
}
