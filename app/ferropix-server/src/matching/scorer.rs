// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Fellegi-Sunter scorer: a weight per field, a total, a score in
//! `[0, 1]` and a grade, with the evidence of every field.
//!
//! For each field, agreement adds `log2(m / u)`, disagreement adds
//! `log2((1 - m) / (1 - u))`, a partial agreement half the agreement weight
//! and an absent field nothing. The total maps to `[0, 1]` by the logistic
//! `1 / (1 + 2^-(total - MIDPOINT_BITS))`, and the grade follows the
//! `[matching]` thresholds. PDQm asks for a score in `[0, 1]` and a grade
//! (PDQm 3.2.0 §2:3.119.4.2.2.4); the weights, the mapping and the grades
//! are our own design (`docs/architecture.md` §5.2).

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

use crate::config::section::matching::{BlockingKey, Field, Matching, Weights};
use crate::matching::compare::{self, Agreement, Comparator};
use crate::matching::subject::Subject;

/// The total, in bits, that scores `0.5`.
///
/// A pair scores above it only when the evidence for it outweighs prior
/// odds of one in 1,024 that two blocked records are one patient.
pub const MIDPOINT_BITS: f64 = 10.0;

/// The share of the agreement weight a partial agreement adds.
pub const PARTIAL_SHARE: f64 = 0.5;

/// Every field the scorer compares, in the order the evidence lists them.
pub const FIELDS: [Field; 10] = [
    Field::FamilyName,
    Field::GivenNames,
    Field::BirthDate,
    Field::Gender,
    Field::PostalCode,
    Field::Address,
    Field::Telephone,
    Field::Email,
    Field::MothersMaidenName,
    Field::LocalIdentifier,
];

/// Returns the default `m` and `u` of `field`, which `[matching.fields]`
/// overrides.
#[must_use]
pub const fn default_weights(field: Field) -> Weights {
    let (m, u) = match field {
        Field::FamilyName => (0.95, 0.01),
        Field::GivenNames => (0.90, 0.02),
        Field::BirthDate => (0.97, 0.003),
        Field::Gender => (0.98, 0.5),
        Field::PostalCode => (0.85, 0.001),
        Field::Address => (0.80, 0.001),
        Field::Telephone | Field::Email => (0.60, 0.0001),
        Field::MothersMaidenName => (0.90, 0.01),
        Field::LocalIdentifier => (0.50, 0.000_01),
    };
    Weights { m, u }
}

/// The grade of a score against the thresholds (PDQm 3.2.0
/// §2:3.119.4.2.2.4, the `match-grade` codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Grade {
    /// Below `possible_at`: not queued.
    CertainlyNot,
    /// At or above `possible_at`: a candidate below the probable ones.
    Possible,
    /// At or above `review_at`: a candidate in the review queue.
    Probable,
    /// At or above `link_at`: linked alone only with `auto_link = true`.
    Certain,
}

impl Grade {
    /// Returns the `match-grade` code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::CertainlyNot => "certainly-not",
            Self::Possible => "possible",
            Self::Probable => "probable",
            Self::Certain => "certain",
        }
    }
}

/// The weights of one field, derived once from its `m` and `u`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FieldWeights {
    /// What agreement adds.
    agree: f64,
    /// What disagreement adds.
    disagree: f64,
}

/// The scorer of one `[matching]` section.
#[derive(Debug, Clone, PartialEq)]
pub struct Scorer {
    /// The weights of each field.
    weights: BTreeMap<Field, FieldWeights>,
    /// The score at or above which a pair grades `certain`.
    link_at: f64,
    /// The score at or above which a pair grades `probable`.
    review_at: f64,
    /// The score at or above which a pair grades `possible`.
    possible_at: f64,
}

impl Scorer {
    /// Returns the scorer of `matching`, a section [`Matching::resolve`]
    /// accepted: each field's override, or its default.
    #[must_use]
    pub fn new(matching: &Matching) -> Self {
        let weights = FIELDS
            .into_iter()
            .map(|field| {
                let Weights { m, u } = matching
                    .fields
                    .get(&field)
                    .copied()
                    .unwrap_or_else(|| default_weights(field));
                let weights = FieldWeights {
                    agree: (m / u).log2(),
                    disagree: ((1.0 - m) / (1.0 - u)).log2(),
                };
                (field, weights)
            })
            .collect();
        Self {
            weights,
            link_at: matching.link_at,
            review_at: matching.review_at,
            possible_at: matching.possible_at,
        }
    }

    /// Returns the agreement and disagreement weight of `field`.
    #[must_use]
    pub fn weights(&self, field: Field) -> Option<(f64, f64)> {
        self.weights.get(&field).map(|w| (w.agree, w.disagree))
    }

    /// Scores the pair `a`, `b` that `key` produced.
    #[must_use]
    pub fn score(&self, a: &Subject, b: &Subject, key: Option<BlockingKey>) -> Scored {
        let mut total = 0.0;
        let mut fields = Vec::with_capacity(FIELDS.len());
        for (field, weights) in &self.weights {
            let comparison = compare::compare(*field, a, b);
            let weight = match comparison.agreement {
                Agreement::Agree => weights.agree,
                Agreement::Partial => weights.agree * PARTIAL_SHARE,
                Agreement::Disagree => weights.disagree,
                Agreement::Absent => 0.0,
            };
            total += weight;
            let (a, b) = comparison
                .values
                .as_ref()
                .map_or((None, None), |(x, y)| (Some(mask(x)), Some(mask(y))));
            fields.push(FieldEvidence {
                field: *field,
                comparator: comparison.comparator,
                a,
                b,
                agreement: comparison.agreement,
                reading: comparison.reading,
                weight,
            });
        }
        let score = 1.0 / (1.0 + (MIDPOINT_BITS - total).exp2());
        let grade = self.grade(score);
        Scored {
            score,
            grade,
            evidence: Evidence {
                blocking_key: key,
                total,
                score,
                grade,
                fields,
            },
        }
    }

    /// Returns the grade of `score`.
    #[must_use]
    pub fn grade(&self, score: f64) -> Grade {
        if score >= self.link_at {
            Grade::Certain
        } else if score >= self.review_at {
            Grade::Probable
        } else if score >= self.possible_at {
            Grade::Possible
        } else {
            Grade::CertainlyNot
        }
    }
}

/// One pair scored.
#[derive(Debug, Clone, PartialEq)]
pub struct Scored {
    /// The score in `[0, 1]`.
    pub score: f64,
    /// The grade of the score.
    pub grade: Grade,
    /// Why the pair scored as it did.
    pub evidence: Evidence,
}

/// The evidence of one score: one entry per field and the blocking key that
/// produced the pair, as the candidate keeps it and the console draws it.
///
/// `Debug` leaves the masked values out.
#[derive(Clone, PartialEq, Serialize)]
pub struct Evidence {
    /// The blocking key that produced the pair, `None` for a pair the
    /// deterministic tier named.
    pub blocking_key: Option<BlockingKey>,
    /// The sum of the weights, in bits.
    pub total: f64,
    /// The score.
    pub score: f64,
    /// The grade.
    pub grade: Grade,
    /// One entry per field, in the order of [`FIELDS`].
    pub fields: Vec<FieldEvidence>,
}

impl fmt::Debug for Evidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Evidence")
            .field("blocking_key", &self.blocking_key)
            .field("total", &self.total)
            .field("score", &self.score)
            .field("grade", &self.grade)
            .field("fields", &self.fields)
            .finish()
    }
}

/// The evidence of one field.
///
/// `Debug` leaves the masked values out.
#[derive(Clone, PartialEq, Serialize)]
pub struct FieldEvidence {
    /// The field.
    pub field: Field,
    /// The comparator it was read with.
    pub comparator: Comparator,
    /// The first record's value as compared, masked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a: Option<String>,
    /// The second record's value as compared, masked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<String>,
    /// How the field compared.
    pub agreement: Agreement,
    /// The comparator's reading, where it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reading: Option<f64>,
    /// The weight the field added, in bits.
    pub weight: f64,
}

impl fmt::Debug for FieldEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldEvidence")
            .field("field", &self.field)
            .field("comparator", &self.comparator)
            .field("agreement", &self.agreement)
            .field("reading", &self.reading)
            .field("weight", &self.weight)
            .finish_non_exhaustive()
    }
}

/// Returns `value` masked: its last three characters, at most half of it,
/// behind a `•` for every other character (`docs/architecture.md` §12.5).
#[must_use]
pub fn mask(value: &str) -> String {
    let length = value.chars().count();
    let shown = (length >> 1).min(3);
    let hidden = length.saturating_sub(shown);
    let mut masked: String = std::iter::repeat_n('•', hidden).collect();
    masked.extend(value.chars().skip(hidden));
    masked
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{FIELDS, Grade, Scorer, default_weights, mask};
    use crate::config::section::matching::{Field, Matching, Weights};
    use crate::matching::subject::{Name, Subject};

    #[test]
    fn the_weights_are_log2_of_m_over_u_and_of_their_complements() {
        let scorer = Scorer::new(&Matching::default());
        let (agree, disagree) = scorer.weights(Field::FamilyName).unwrap();
        assert!((agree - (0.95_f64 / 0.01).log2()).abs() < 1e-12, "{agree}");
        assert!(
            (disagree - (0.05_f64 / 0.99).log2()).abs() < 1e-12,
            "{disagree}"
        );
    }

    #[test]
    fn every_default_holds_zero_u_m_one() {
        for field in FIELDS {
            let Weights { m, u } = default_weights(field);
            assert!(0.0 < u && u < m && m < 1.0, "{field:?}");
        }
    }

    #[test]
    fn an_override_replaces_the_default_of_its_field_alone() {
        let matching = Matching {
            fields: BTreeMap::from([(Field::Gender, Weights { m: 0.9, u: 0.45 })]),
            ..Matching::default()
        };
        let scorer = Scorer::new(&matching);
        let (agree, _) = scorer.weights(Field::Gender).unwrap();
        assert!((agree - 2.0_f64.log2()).abs() < 1e-12, "{agree}");
        assert_eq!(
            Scorer::new(&Matching::default()).weights(Field::FamilyName),
            scorer.weights(Field::FamilyName)
        );
    }

    #[test]
    fn two_empty_subjects_score_the_midpoint_s_logistic_with_every_field_absent() {
        let scored =
            Scorer::new(&Matching::default()).score(&Subject::default(), &Subject::default(), None);
        assert_eq!(10, scored.evidence.fields.len());
        assert!(
            scored.evidence.fields.iter().all(|f| f.weight == 0.0),
            "no weight"
        );
        assert!(
            (scored.score - 1.0 / (1.0 + 1024.0)).abs() < 1e-12,
            "{}",
            scored.score
        );
        assert_eq!(Grade::CertainlyNot, scored.grade);
    }

    #[test]
    fn the_grade_follows_each_threshold() {
        let scorer = Scorer::new(&Matching::default());
        for (score, grade) in [
            (0.95, Grade::Certain),
            (0.949, Grade::Probable),
            (0.80, Grade::Probable),
            (0.60, Grade::Possible),
            (0.599, Grade::CertainlyNot),
        ] {
            assert_eq!(grade, scorer.grade(score), "{score}");
        }
    }

    #[test]
    fn a_value_is_masked_to_at_most_three_characters_and_half_its_length() {
        assert_eq!("•••••sma", mask("proefsma"));
        assert_eq!("•••-14", mask("958-14"));
        assert_eq!("••a", mask("ada"));
        assert_eq!("•", mask("a"));
        assert_eq!("", mask(""));
    }

    #[test]
    fn the_evidence_debug_shows_no_value() {
        let family = ferropix_testkit::synthetic::FAMILY_NAMES[3].to_lowercase();
        let named = Subject {
            names: vec![Name {
                family: Some(family.clone()),
                given: Vec::new(),
            }],
            ..Subject::default()
        };
        let scored = Scorer::new(&Matching::default()).score(&named, &named, None);
        let rendered = format!("{:?}", scored.evidence);
        let tail: String = family
            .chars()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert!(!rendered.contains(&tail), "{rendered}");
    }
}
