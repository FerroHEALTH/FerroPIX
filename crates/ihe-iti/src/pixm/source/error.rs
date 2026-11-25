// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why an ITI-104 message was not applied.
//!
//! No error here carries a request URL or body, an identifier value, a
//! demographic, or text the Manager wrote: the URL holds the identifier, the
//! body the Patient, and the Manager's `OperationOutcome` may quote either.

use ferrohealth_smart::client::outbound::SendError;
use http::StatusCode;

use crate::exchange::Unanswered;
use crate::outbound::Unauthenticated;
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

/// Why an ITI-104 message did not end in the Manager applying it.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SourceError {
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
    /// The Manager answered a status that does not apply the message, with
    /// the issue types its `OperationOutcome` carried, if it sent one.
    #[error("the PIX Manager answered {status}")]
    Rejected {
        /// The HTTP status.
        status: StatusCode,
        /// The issue types of the `OperationOutcome`, in order.
        issues: Vec<IssueType>,
    },
    /// No answer arrived within the timeout.
    #[error("the PIX Manager did not answer within the timeout")]
    Timeout,
    /// The request could not be sent or the answer could not be read.
    #[error("the PIX Manager could not be reached")]
    Transport(#[source] reqwest::Error),
    /// No token could be obtained for the request, or a refused one renewed
    /// (IUA ITI-72 §3.72.4.3).
    #[error("the request to the PIX Manager could not be authenticated")]
    Unauthenticated(#[source] SendError),
    /// The audit recorder could not accept the exchange's audit record, so
    /// its outcome is not used (feature `balp`, §2:3.104.5.1.1).
    #[cfg(feature = "balp")]
    #[error("the ITI-104 audit record could not be recorded")]
    Audit(#[source] ihe_atna::balp::AuditError),
}

impl Unanswered for SourceError {
    fn timeout() -> Self {
        Self::Timeout
    }

    fn transport(source: reqwest::Error) -> Self {
        Self::Transport(source)
    }
}

impl Unauthenticated for SourceError {
    fn unauthenticated(source: SendError) -> Self {
        Self::Unauthenticated(source)
    }
}
