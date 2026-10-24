// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Patient Demographics Supplier side of PDQm 3.2.0 (feature
//! `pdqm-server`).
//!
//! The Supplier of ITI-78, Mobile Patient Demographics Query, and of ITI-119,
//! Patient Demographics Match, typed and with no store and no HTTP server:
//!
//! - [`search::PatientSearch`] reads the search parameters of ITI-78
//!   (§2:3.78.4.1.2.1) from the query of a `GET` or the form body of a
//!   `POST`, with the `:exact` modifier (§2:3.78.4.1.2.2), AND and OR, and
//!   the domain filter (§2:3.78.4.1.2.3), into one typed query that says
//!   which parameters it dropped and writes the `self` link of what it
//!   respected (§2:3.78.4.1.2.1.1); [`searchset::searchset`] writes the
//!   `searchset` Bundle with its `total` and paging links (§2:3.78.4.2.2).
//! - [`matching::MatchRequest`] reads the `$match` input, the PDQm Match
//!   Input `Parameters` or a bare `Patient` (§2:3.119.4.1.2), and
//!   [`matching::match_set`] writes the Match Output Bundle, each entry with
//!   its `search.score` and `match-grade` (§2:3.119.4.2.2.4).
//! - [`Refusal`] is each failure the transactions name with a status, and
//!   [`retrieved`] the answer of the retrieve (§2:3.78.4.3).
//!
//! The store, the matching and the router are the caller's. Every
//! demographic travels in a redacting type: no type here has `Display`,
//! every `Debug` leaves values out, and no error or outcome quotes the
//! request.
//!
//! # Examples
//!
//! ```
//! use ihe_iti::pdqm::server::search::PatientSearch;
//! use ihe_iti::pdqm::server::searchset::{Paging, searchset};
//! use url::Url;
//!
//! let search = PatientSearch::from_query("family:exact=MOHR&gender=female&_sort=given")?;
//! assert_eq!(search.dropped(), ["_sort"]);
//! let endpoint = Url::parse("https://pdq.example.org/fhir/Patient")?;
//! let answer = searchset(&search, &endpoint, 0, Vec::new(), &Paging::new())?;
//! assert_eq!(answer.status(), http::StatusCode::OK);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod matching;
pub mod search;
pub mod searchset;

use std::fmt;

use fhir_types::r4::operation_outcome::OperationOutcome;
use fhir_types::r4::patient::Patient;
use http::StatusCode;

use crate::outcome::IssueType;
use crate::server::answer::{Answer, Severity};

/// A failure ITI-78 names, which the Supplier decides from its store.
///
/// The format failure of Case 5 is
/// [`Unacceptable`](crate::server::format::Unacceptable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The Supplier is not an authority for a domain the domain filter
    /// names: the preferred `404` of §2:3.78.4.1.3, Case 4.
    DomainNotRecognized,
    /// No Patient has the id a retrieve names (§2:3.78.4.3.3, Case 2).
    PatientNotFound,
}

impl Refusal {
    /// Returns the answer the transaction prescribes: its status, and an
    /// `OperationOutcome` with the issue's severity, code and diagnostics.
    #[must_use]
    pub fn answer(self) -> Answer<OperationOutcome> {
        match self {
            Self::DomainNotRecognized => Answer::outcome(
                StatusCode::NOT_FOUND,
                Severity::Warning,
                IssueType::NotFound,
                "targetSystem not found",
            ),
            Self::PatientNotFound => Answer::outcome(
                StatusCode::NOT_FOUND,
                Severity::Error,
                IssueType::NotFound,
                "the Patient Resource could not be found",
            ),
        }
    }
}

/// Returns the `200` answer of a retrieve: the one Patient the id names
/// (§2:3.78.4.3.3, Case 1).
#[must_use]
pub fn retrieved(patient: Patient) -> Answer<Patient> {
    Answer::new(StatusCode::OK, patient)
}

/// The Supplier's confidence in a match: a decimal between 0 and 1
/// (§2:3.78.4.2.2.5, §2:3.119.4.1.3; FHIR R4 search, "Relevance",
/// <http://hl7.org/fhir/R4/search.html#score>).
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub struct Score(f64);

impl Score {
    /// Creates the score `value`.
    ///
    /// # Errors
    /// [`ScoreError`] when `value` is not a finite number between 0 and 1.
    pub fn new(value: f64) -> Result<Self, ScoreError> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ScoreError)
        }
    }

    /// Returns the score.
    #[must_use]
    pub fn value(self) -> f64 {
        self.0
    }

    /// The score as a FHIR `decimal`: the shortest text that reads back as
    /// the same number.
    pub(super) fn decimal(self) -> String {
        self.0.to_string()
    }
}

impl fmt::Debug for Score {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Score({})", self.0)
    }
}

/// A score outside 0 to 1, or not a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a score is a number between 0 and 1")]
pub struct ScoreError;

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use super::{Refusal, Score, ScoreError};
    use crate::outcome::IssueType;

    #[test]
    fn a_score_lies_between_zero_and_one() {
        assert_eq!(Score::new(0.9).map(Score::decimal), Ok("0.9".to_owned()));
        assert_eq!(Score::new(1.0).map(Score::decimal), Ok("1".to_owned()));
        for bad in [-0.1, 1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(Score::new(bad).err(), Some(ScoreError), "{bad}");
        }
    }

    #[test]
    fn case_four_is_a_warning_and_a_missing_patient_an_error() {
        let domain = Refusal::DomainNotRecognized.answer();
        assert_eq!(domain.status(), StatusCode::NOT_FOUND);
        assert_eq!(domain.issue_type(), IssueType::NotFound);
        assert_eq!(
            domain.resource().issue[0].severity.value.as_deref(),
            Some("warning"),
            "§2:3.78.4.1.3 Case 4"
        );
        let missing = Refusal::PatientNotFound.answer();
        assert_eq!(
            missing.resource().issue[0].severity.value.as_deref(),
            Some("error"),
            "§2:3.78.4.3.3 Case 2"
        );
    }
}
