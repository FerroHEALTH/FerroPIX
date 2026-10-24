// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The PIX Manager side of PIXm 3.1.0 (feature `pixm-server`).
//!
//! The Patient Identifier Cross-reference Manager of ITI-83, Mobile Patient
//! Identifier Cross-reference Query, and of ITI-104, Patient Identity Feed
//! FHIR, typed and with no store and no HTTP server.
//!
//! - [`query::PixQuery`] reads one `$ihe-pix` request, from the query of the
//!   `GET` ITI-83 prescribes (§2:3.83.4.1.2) or from the `Parameters` body of
//!   the `POST` FHIR R4 gives every operation (FHIR R4 Operations,
//!   <http://hl7.org/fhir/R4/operations.html#executing>), into one request
//!   type; [`answer::cross_references`] writes the `Parameters` answer
//!   (§2:3.83.4.2.2.1) and [`Refusal`] the `OperationOutcome` of each failure
//!   the transaction names (§2:3.83.4.2.2.2 to §2:3.83.4.2.2.4).
//! - [`feed::Feed`] reads the conditional `PUT` on
//!   `Patient?identifier=system|value` that adds, revises or resolves a
//!   duplicate (§2:3.104.4.1.2, §2:3.104.4.2.2), and [`feed::Removal`] the
//!   conditional `DELETE` of the Remove Patient Option (§2:3.104.4.3.2).
//!
//! The store, the cross-referencing and the router are the caller's: a
//! reader hands back what was asked, a writer what to send, each refusal an
//! [`Answer`] with its status and `OperationOutcome`.
//!
//! Every identifier travels in the redacting types of the client side
//! ([`crate::pixm::identifier`]) and every Patient behind a redacting
//! wrapper: no type here has `Display`, every `Debug` leaves values out, and
//! no error or outcome quotes the request.
//!
//! # Examples
//!
//! ```
//! use ihe_iti::pixm::identifier::TargetIdentifier;
//! use ihe_iti::pixm::server::answer::cross_references;
//! use ihe_iti::pixm::server::query::PixQuery;
//! use ihe_iti::server::format::Format;
//! use secrecy::{ExposeSecret, SecretString};
//!
//! let query = PixQuery::from_query(
//!     "sourceIdentifier=urn:oid:2.999.1%7CIHERED-994&targetSystem=urn:oid:2.999.3",
//! )?;
//! assert_eq!(query.source().value().expose_secret(), "IHERED-994");
//! let found = [TargetIdentifier::new("urn:oid:2.999.3", SecretString::from("IHEBLUE-994"))?];
//! let answer = cross_references(&query, &found, &[]);
//! assert_eq!(answer.status(), http::StatusCode::OK);
//! let _body = answer.encode(Format::Json)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod answer;
pub mod feed;
pub mod query;

use fhir_types::r4::operation_outcome::OperationOutcome;
use http::StatusCode;

use crate::outcome::IssueType;
use crate::server::answer::{Answer, Severity};

/// A failure ITI-83 names, which the Manager decides from its store
/// (§2:3.83.4.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The Manager knows the source identifier's domain and not the
    /// identifier, or the identifier was removed (§2:3.83.4.2.2.2,
    /// §2:3.104.4.3.3).
    SourceIdentifierNotFound,
    /// The Manager does not know the source identifier's domain
    /// (§2:3.83.4.2.2.3).
    SourceDomainNotRecognized,
    /// The Manager does not know one or more target domains
    /// (§2:3.83.4.2.2.4).
    TargetDomainNotRecognized,
}

impl Refusal {
    /// Returns the answer the transaction prescribes: its status, and an
    /// `OperationOutcome` with the issue's severity, code and diagnostics.
    #[must_use]
    pub fn answer(self) -> Answer<OperationOutcome> {
        match self {
            Self::SourceIdentifierNotFound => Answer::outcome(
                StatusCode::NOT_FOUND,
                Severity::Error,
                IssueType::NotFound,
                "sourceIdentifier Patient Identifier not found",
            ),
            Self::SourceDomainNotRecognized => Answer::outcome(
                StatusCode::BAD_REQUEST,
                Severity::Error,
                IssueType::CodeInvalid,
                "sourceIdentifier Assigning Authority not found",
            ),
            Self::TargetDomainNotRecognized => Answer::outcome(
                StatusCode::FORBIDDEN,
                Severity::Error,
                IssueType::CodeInvalid,
                "targetSystem not found",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use super::Refusal;
    use crate::outcome::IssueType;

    #[test]
    fn each_refusal_is_the_one_the_transaction_names() {
        for (refusal, status, code) in [
            (
                Refusal::SourceIdentifierNotFound,
                StatusCode::NOT_FOUND,
                IssueType::NotFound,
            ),
            (
                Refusal::SourceDomainNotRecognized,
                StatusCode::BAD_REQUEST,
                IssueType::CodeInvalid,
            ),
            (
                Refusal::TargetDomainNotRecognized,
                StatusCode::FORBIDDEN,
                IssueType::CodeInvalid,
            ),
        ] {
            let answer = refusal.answer();
            assert_eq!(answer.status(), status, "{refusal:?}");
            assert_eq!(answer.issue_type(), code, "{refusal:?}");
        }
    }
}
