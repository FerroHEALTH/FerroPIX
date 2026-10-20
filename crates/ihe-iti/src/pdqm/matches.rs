// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The result set of an ITI-78 search: one page of matching Patients, the
//! total the Supplier counts, and the link to the next page (§2:3.78.4.2.2);
//! and the answer to an ITI-119 match (§2:3.119.4.2.2).
//!
//! Every Patient here is demographic data. The types redact it in `Debug` and
//! none of them has `Display`.

use std::fmt;

use fhir_types::r4::patient::Patient;
use secrecy::SecretString;
use url::Url;

use crate::outcome::IssueType;
use crate::redact::REDACTED;

/// One page of an ITI-78 result set.
#[derive(Debug, Clone)]
pub struct SearchResult {
    total: u32,
    patients: Vec<MatchedPatient>,
    warnings: Vec<IssueType>,
    next: Option<Page>,
}

impl SearchResult {
    pub(super) fn new(
        total: u32,
        patients: Vec<MatchedPatient>,
        warnings: Vec<IssueType>,
        next: Option<Page>,
    ) -> Self {
        Self {
            total,
            patients,
            warnings,
            next,
        }
    }

    /// Returns the number of Patients the whole result set matches, over every
    /// page (`Bundle.total`); `0` is the profile's no-match answer
    /// (§2:3.78.4.1.3, Case 3).
    #[must_use]
    pub fn total(&self) -> u32 {
        self.total
    }

    /// Returns the matching Patients of this page, in the Supplier's order.
    #[must_use]
    pub fn patients(&self) -> &[MatchedPatient] {
        &self.patients
    }

    /// Returns the matching Patients of this page, by value.
    #[must_use]
    pub fn into_patients(self) -> Vec<MatchedPatient> {
        self.patients
    }

    /// Returns the issue types of the `OperationOutcome` entries the page
    /// carries, such as the `not-found` warning of an identifier domain the
    /// Supplier does not recognise (§2:3.78.4.1.3, Case 4).
    #[must_use]
    pub fn warnings(&self) -> &[IssueType] {
        &self.warnings
    }

    /// Returns the link to the next page, when the Supplier pages the result
    /// set (§2:3.78.4.2.2.4).
    #[must_use]
    pub fn next(&self) -> Option<&Page> {
        self.next.as_ref()
    }
}

/// A Patient the search matched, with the Supplier's quality of match.
///
/// A deprecated Patient is matched too, with `active` set to `false` and a
/// `replaced-by` link to the Patient that replaces it (§2:3.78.4.1.3, Case 6).
#[derive(Clone)]
pub struct MatchedPatient {
    full_url: SecretString,
    patient: Box<Patient>,
    score: Option<f64>,
    grade: Option<MatchGrade>,
}

impl MatchedPatient {
    pub(super) fn new(
        full_url: SecretString,
        patient: Box<Patient>,
        score: Option<f64>,
        grade: Option<MatchGrade>,
    ) -> Self {
        Self {
            full_url,
            patient,
            score,
            grade,
        }
    }

    /// Returns the entry's `fullUrl`: the Patient's absolute URL on the
    /// Supplier.
    #[must_use]
    pub fn full_url(&self) -> &SecretString {
        &self.full_url
    }

    /// Returns the Patient resource, decoded as FHIR R4.
    ///
    /// It is not held to the PDQm Patient profile: the Consumer "SHOULD be
    /// robust" to a matching Patient that does not conform to it
    /// (§2:3.78.4.2.3).
    #[must_use]
    pub fn patient(&self) -> &Patient {
        &self.patient
    }

    /// Returns the Supplier's confidence in the match, `entry.search.score`,
    /// when it conveys one (§2:3.78.4.2.2.5).
    #[must_use]
    pub fn score(&self) -> Option<f64> {
        self.score
    }

    /// Returns the `match-grade` extension of `entry.search`, when the
    /// Supplier states one.
    #[must_use]
    pub fn grade(&self) -> Option<MatchGrade> {
        self.grade
    }
}

impl fmt::Debug for MatchedPatient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MatchedPatient")
            .field("full_url", &REDACTED)
            .field("patient", &REDACTED)
            .field("score", &self.score)
            .field("grade", &self.grade)
            .finish()
    }
}

/// The FHIR R4 `match-grade` of a match
/// (<http://hl7.org/fhir/R4/extension-match-grade.html>, a required binding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatchGrade {
    /// `certain`
    Certain,
    /// `probable`
    Probable,
    /// `possible`
    Possible,
    /// `certainly-not`
    CertainlyNot,
}

impl MatchGrade {
    /// The canonical URL of the extension.
    pub(super) const EXTENSION: &str = "http://hl7.org/fhir/StructureDefinition/match-grade";

    /// Returns the grade `code` names, or `None` for a code outside the value
    /// set.
    pub(super) fn from_code(code: &str) -> Option<Self> {
        match code {
            "certain" => Some(Self::Certain),
            "probable" => Some(Self::Probable),
            "possible" => Some(Self::Possible),
            "certainly-not" => Some(Self::CertainlyNot),
            _ => None,
        }
    }
}

/// The link to the next page of a result set.
///
/// The Supplier writes the link and may put the query into it, so `Debug`
/// redacts it and the type has no `Display`.
#[derive(Clone)]
pub struct Page(Url);

impl Page {
    pub(super) fn new(url: Url) -> Self {
        Self(url)
    }

    pub(super) fn url(&self) -> &Url {
        &self.0
    }
}

impl fmt::Debug for Page {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Page({REDACTED})")
    }
}

/// The answer to an ITI-119 match: the Patients the Supplier matched, most
/// likely first, each with its score and grade (§2:3.119.4.1.3, Cases 1 to 6;
/// §2:3.119.4.2.2.4).
///
/// No match is an answer with no Patient (§2:3.119.4.1.3, Cases 4, 5 and 7).
#[derive(Debug, Clone)]
pub struct MatchResult {
    patients: Vec<MatchedPatient>,
    warnings: Vec<IssueType>,
}

impl MatchResult {
    pub(super) fn new(patients: Vec<MatchedPatient>, warnings: Vec<IssueType>) -> Self {
        Self { patients, warnings }
    }

    /// Returns the matched Patients in the Supplier's order, most likely
    /// first (§2:3.119.4.1.3, Case 2); each carries a score and a grade.
    #[must_use]
    pub fn patients(&self) -> &[MatchedPatient] {
        &self.patients
    }

    /// Returns the matched Patients, by value.
    #[must_use]
    pub fn into_patients(self) -> Vec<MatchedPatient> {
        self.patients
    }

    /// Returns the issue types of the warning `OperationOutcome` entries the
    /// answer carries (§2:3.119.4.1.3, Case 10).
    #[must_use]
    pub fn warnings(&self) -> &[IssueType] {
        &self.warnings
    }
}
