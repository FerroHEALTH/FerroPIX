// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The one status table of the FHIR face: every outcome a transaction
//! answers with an `OperationOutcome`, its HTTP status, the issue's
//! severity, its code and its diagnostics (`docs/architecture.md` §6).
//!
//! A case a profile names is answered as `ihe-iti` writes it, so every face
//! that meets the case answers the same code and issue; the cases no
//! profile names are FerroPIX's own and say so. The guard's `401` and `403`
//! live with the guard (`crate::auth::guard`). Every diagnostics text is
//! fixed: no identifier, value or name from the request reaches it.

use fhir_types::r4::operation_outcome::OperationOutcome;
use http::StatusCode;
use ihe_iti::outcome::IssueType;
use ihe_iti::pixm::server::Refusal;
use ihe_iti::server::answer::{Answer, Severity};
use ihe_iti::server::format::{Format, Unacceptable};

use crate::audit::reserve::UNAUDITED;

/// One row of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Case {
    /// ITI-83: the domain is known and the identifier is not, or was
    /// removed (PIXm 3.1.0 §2:3.83.4.2.2.2, §2:3.104.4.3.3).
    SourceIdentifierNotFound,
    /// ITI-83: the source domain is unknown or retired (§2:3.83.4.2.2.3).
    SourceDomainNotRecognized,
    /// ITI-83: a target domain is unknown or retired (§2:3.83.4.2.2.4).
    TargetDomainNotRecognized,
    /// ITI-83: the identifier has no record of its own and the records that
    /// carry it are in more than one person (#167). No profile names it:
    /// our own design.
    SourceIdentifierAmbiguous,
    /// ITI-104: an Add or Revise Patient message created the record
    /// (FHIR R4 http.html §3.1.0.4, `201`).
    FeedCreated,
    /// ITI-104: an Add or Revise Patient message revised the record, or a
    /// Resolve Duplicate Patient message subsumed it (§3.1.0.4, `200`).
    FeedUpdated,
    /// ITI-104: a Remove Patient message removed the identifier
    /// (§3.1.0.7, `200`).
    FeedRemoved,
    /// ITI-104: a Remove Patient message named an identifier the
    /// cross-reference does not hold, which FHIR R4 answers as a delete of a
    /// resource that does not exist (§3.1.0.7, `200`).
    FeedNothingToRemove,
    /// ITI-104: the condition names a domain the Manager does not know or
    /// retired. No profile names it: our own design, after ITI-83's
    /// §2:3.83.4.2.2.3.
    FeedDomainNotRecognized,
    /// ITI-104: an identifier's value is refused by its domain's rule, a
    /// server business rule (FHIR R4 http.html §3.1.0.4.2, `422`).
    FeedIdentifierRefused,
    /// ITI-104: the condition names an identifier the Source subsumed,
    /// which takes no new version (§2:3.104.4.2.3; `422`, §3.1.0.4.2).
    FeedSubsumed,
    /// ITI-104: the surviving identifier of a Resolve Duplicate Patient
    /// message names no active record of the domain (§2:3.104.4.2.3;
    /// `422`, §3.1.0.4.2).
    FeedSurvivorNotFound,
    /// Any route: no FHIR encoding the request accepts can be written
    /// (PDQm 3.2.0 §2:3.78.4.1.3, Case 5, the answer every face shares).
    NotAcceptable,
    /// Any route with a body: the body is neither FHIR JSON nor FHIR XML
    /// (FHIR R4 http.html §3.1.0.1.9, `415`).
    UnsupportedMediaType,
    /// Any route with a body: the body exceeds `[server] body_limit_bytes`.
    /// Our own design.
    PayloadTooLarge,
    /// Any route: the cross-reference store could not be read or written.
    /// Our own design.
    StoreUnavailable,
    /// ITI-104: concurrent feeds moved the persons this one joins on every
    /// attempt. Our own design.
    Contention,
    /// Any audited route: the record could not be stored, so nothing was
    /// answered or changed. Our own design (`crate::audit`).
    Unaudited,
    /// ITI-104: the change committed and its audit record could not be
    /// stored. Our own design.
    CommittedUnaudited,
    /// Any route: a fault of the server itself. Our own design.
    Internal,
}

impl Case {
    /// Every row, in the order of the table.
    pub const ALL: [Self; 20] = [
        Self::SourceIdentifierNotFound,
        Self::SourceDomainNotRecognized,
        Self::TargetDomainNotRecognized,
        Self::SourceIdentifierAmbiguous,
        Self::FeedCreated,
        Self::FeedUpdated,
        Self::FeedRemoved,
        Self::FeedNothingToRemove,
        Self::FeedDomainNotRecognized,
        Self::FeedIdentifierRefused,
        Self::FeedSubsumed,
        Self::FeedSurvivorNotFound,
        Self::NotAcceptable,
        Self::UnsupportedMediaType,
        Self::PayloadTooLarge,
        Self::StoreUnavailable,
        Self::Contention,
        Self::Unaudited,
        Self::CommittedUnaudited,
        Self::Internal,
    ];

    /// Returns the answer of this case: its status and its
    /// `OperationOutcome`.
    #[must_use]
    pub fn answer(self) -> Answer<OperationOutcome> {
        let error =
            |status, code, diagnostics| Answer::outcome(status, Severity::Error, code, diagnostics);
        let information = |status, diagnostics| {
            Answer::outcome(
                status,
                Severity::Information,
                IssueType::Informational,
                diagnostics,
            )
        };
        match self {
            Self::SourceIdentifierNotFound => Refusal::SourceIdentifierNotFound.answer(),
            Self::SourceDomainNotRecognized => Refusal::SourceDomainNotRecognized.answer(),
            Self::TargetDomainNotRecognized => Refusal::TargetDomainNotRecognized.answer(),
            Self::SourceIdentifierAmbiguous => error(
                StatusCode::CONFLICT,
                IssueType::MultipleMatches,
                "sourceIdentifier is cross-referenced to more than one patient pending review",
            ),
            Self::FeedCreated => information(
                StatusCode::CREATED,
                "the identifier is added to the cross-reference",
            ),
            Self::FeedUpdated => information(
                StatusCode::OK,
                "the identifier's record in the cross-reference is revised",
            ),
            Self::FeedRemoved => information(
                StatusCode::OK,
                "the identifier is removed from the cross-reference",
            ),
            Self::FeedNothingToRemove => information(
                StatusCode::OK,
                "the cross-reference holds no such identifier",
            ),
            Self::FeedDomainNotRecognized => error(
                StatusCode::BAD_REQUEST,
                IssueType::CodeInvalid,
                "identifier Assigning Authority not found",
            ),
            Self::FeedIdentifierRefused => error(
                StatusCode::UNPROCESSABLE_ENTITY,
                IssueType::BusinessRule,
                "an identifier's value is refused by the rule of its domain",
            ),
            Self::FeedSubsumed => error(
                StatusCode::UNPROCESSABLE_ENTITY,
                IssueType::BusinessRule,
                "the identifier was subsumed by a resolved duplicate and takes no revision",
            ),
            Self::FeedSurvivorNotFound => error(
                StatusCode::UNPROCESSABLE_ENTITY,
                IssueType::BusinessRule,
                "the surviving identifier names no active record of the domain",
            ),
            Self::NotAcceptable => Unacceptable.refusal(),
            Self::UnsupportedMediaType => Format::unsupported_media_type(),
            Self::PayloadTooLarge => error(
                StatusCode::PAYLOAD_TOO_LARGE,
                IssueType::TooCostly,
                "the request body is larger than this server accepts",
            ),
            Self::StoreUnavailable => error(
                StatusCode::SERVICE_UNAVAILABLE,
                IssueType::Transient,
                "the cross-reference store could not be reached; send the request again",
            ),
            Self::Contention => error(
                StatusCode::SERVICE_UNAVAILABLE,
                IssueType::Transient,
                "concurrent feeds changed the persons this feed joins; send it again",
            ),
            Self::Unaudited => error(
                StatusCode::SERVICE_UNAVAILABLE,
                IssueType::Transient,
                UNAUDITED,
            ),
            Self::CommittedUnaudited => error(
                StatusCode::INTERNAL_SERVER_ERROR,
                IssueType::Exception,
                "the change is committed and its audit record could not be stored",
            ),
            Self::Internal => error(
                StatusCode::INTERNAL_SERVER_ERROR,
                IssueType::Exception,
                "the server could not complete the request",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use http::StatusCode;
    use ihe_iti::outcome::IssueType;

    use super::Case;

    /// Each row as the cited section has it: status, severity, issue code
    /// and diagnostics.
    const TABLE: [(Case, StatusCode, &str, IssueType, &str); 20] = [
        // PIXm 3.1.0 §2:3.83.4.2.2.2
        (
            Case::SourceIdentifierNotFound,
            StatusCode::NOT_FOUND,
            "error",
            IssueType::NotFound,
            "sourceIdentifier Patient Identifier not found",
        ),
        // §2:3.83.4.2.2.3
        (
            Case::SourceDomainNotRecognized,
            StatusCode::BAD_REQUEST,
            "error",
            IssueType::CodeInvalid,
            "sourceIdentifier Assigning Authority not found",
        ),
        // §2:3.83.4.2.2.4
        (
            Case::TargetDomainNotRecognized,
            StatusCode::FORBIDDEN,
            "error",
            IssueType::CodeInvalid,
            "targetSystem not found",
        ),
        (
            Case::SourceIdentifierAmbiguous,
            StatusCode::CONFLICT,
            "error",
            IssueType::MultipleMatches,
            "sourceIdentifier is cross-referenced to more than one patient pending review",
        ),
        // FHIR R4 http.html §3.1.0.4
        (
            Case::FeedCreated,
            StatusCode::CREATED,
            "information",
            IssueType::Informational,
            "the identifier is added to the cross-reference",
        ),
        (
            Case::FeedUpdated,
            StatusCode::OK,
            "information",
            IssueType::Informational,
            "the identifier's record in the cross-reference is revised",
        ),
        // §3.1.0.7
        (
            Case::FeedRemoved,
            StatusCode::OK,
            "information",
            IssueType::Informational,
            "the identifier is removed from the cross-reference",
        ),
        (
            Case::FeedNothingToRemove,
            StatusCode::OK,
            "information",
            IssueType::Informational,
            "the cross-reference holds no such identifier",
        ),
        (
            Case::FeedDomainNotRecognized,
            StatusCode::BAD_REQUEST,
            "error",
            IssueType::CodeInvalid,
            "identifier Assigning Authority not found",
        ),
        // §3.1.0.4.2
        (
            Case::FeedIdentifierRefused,
            StatusCode::UNPROCESSABLE_ENTITY,
            "error",
            IssueType::BusinessRule,
            "an identifier's value is refused by the rule of its domain",
        ),
        (
            Case::FeedSubsumed,
            StatusCode::UNPROCESSABLE_ENTITY,
            "error",
            IssueType::BusinessRule,
            "the identifier was subsumed by a resolved duplicate and takes no revision",
        ),
        (
            Case::FeedSurvivorNotFound,
            StatusCode::UNPROCESSABLE_ENTITY,
            "error",
            IssueType::BusinessRule,
            "the surviving identifier names no active record of the domain",
        ),
        // PDQm 3.2.0 §2:3.78.4.1.3, Case 5
        (
            Case::NotAcceptable,
            StatusCode::NOT_ACCEPTABLE,
            "error",
            IssueType::NotSupported,
            "the requested response format is not supported",
        ),
        // FHIR R4 http.html §3.1.0.1.9
        (
            Case::UnsupportedMediaType,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "error",
            IssueType::NotSupported,
            "the request body is neither FHIR JSON nor FHIR XML",
        ),
        (
            Case::PayloadTooLarge,
            StatusCode::PAYLOAD_TOO_LARGE,
            "error",
            IssueType::TooCostly,
            "the request body is larger than this server accepts",
        ),
        (
            Case::StoreUnavailable,
            StatusCode::SERVICE_UNAVAILABLE,
            "error",
            IssueType::Transient,
            "the cross-reference store could not be reached; send the request again",
        ),
        (
            Case::Contention,
            StatusCode::SERVICE_UNAVAILABLE,
            "error",
            IssueType::Transient,
            "concurrent feeds changed the persons this feed joins; send it again",
        ),
        (
            Case::Unaudited,
            StatusCode::SERVICE_UNAVAILABLE,
            "error",
            IssueType::Transient,
            "the audit record of this request could not be stored, so the request was not processed",
        ),
        (
            Case::CommittedUnaudited,
            StatusCode::INTERNAL_SERVER_ERROR,
            "error",
            IssueType::Exception,
            "the change is committed and its audit record could not be stored",
        ),
        (
            Case::Internal,
            StatusCode::INTERNAL_SERVER_ERROR,
            "error",
            IssueType::Exception,
            "the server could not complete the request",
        ),
    ];

    #[test]
    fn every_row_answers_its_status_severity_code_and_diagnostics() {
        assert_eq!(
            Case::ALL.to_vec(),
            TABLE.iter().map(|row| row.0).collect::<Vec<_>>(),
            "the test holds every row once"
        );
        for (case, status, severity, code, diagnostics) in TABLE {
            let answer = case.answer();
            assert_eq!(status, answer.status(), "{case:?}");
            assert_eq!(code, answer.issue_type(), "{case:?}");
            let issue = answer.resource().issue.first().expect("one issue");
            assert_eq!(Some(severity), issue.severity.value.as_deref(), "{case:?}");
            assert_eq!(
                Some(diagnostics),
                issue.diagnostics.as_ref().and_then(|d| d.value.as_deref()),
                "{case:?}"
            );
            assert_eq!(1, answer.resource().issue.len(), "{case:?}");
        }
    }
}
