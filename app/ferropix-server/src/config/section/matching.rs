// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[matching]` section: the grade thresholds, the per-field weights the
//! scorer's defaults give way to, the blocking keys and `auto_link`.
//!
//! No specification governs the matching: PIXm places it outside IHE (PIXm
//! 3.1.0 §2:3.104.4.1.3) and PDQm asks only for a score in `[0, 1]` and a
//! grade (PDQm 3.2.0 §2:3.119.4.2.2.4). Our own design.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::error::Error;

/// A field the scorer compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    /// The family name.
    FamilyName,
    /// The given names.
    GivenNames,
    /// The birth date.
    BirthDate,
    /// The administrative gender.
    Gender,
    /// The postal code.
    PostalCode,
    /// The address line and city.
    Address,
    /// The telephone number.
    Telephone,
    /// The email address.
    Email,
    /// The mother's maiden name.
    MothersMaidenName,
    /// An identifier in a `local` domain both records' domains share.
    LocalIdentifier,
}

impl Field {
    /// Returns the key the field is written under in `[matching.fields]`.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::FamilyName => "family_name",
            Self::GivenNames => "given_names",
            Self::BirthDate => "birth_date",
            Self::Gender => "gender",
            Self::PostalCode => "postal_code",
            Self::Address => "address",
            Self::Telephone => "telephone",
            Self::Email => "email",
            Self::MothersMaidenName => "mothers_maiden_name",
            Self::LocalIdentifier => "local_identifier",
        }
    }
}

/// A key a pair of records must share to be scored at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockingKey {
    /// The birth year and the phonetic key of the family name.
    BirthYearFamilyPhonetic,
    /// The postal code and the birth date.
    PostalCodeBirthDate,
    /// An identifier value shared across domains.
    SharedIdentifier,
}

/// The Fellegi-Sunter probabilities of one field.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Weights {
    /// The probability that the field agrees for a true pair.
    pub m: f64,
    /// The probability that the field agrees for a random pair.
    pub u: f64,
}

/// The `[matching]` section as written.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Matching {
    /// The score at or above which a pair grades `certain`.
    pub link_at: f64,
    /// The score at or above which a pair grades `probable`.
    pub review_at: f64,
    /// The score at or above which a pair grades `possible`.
    pub possible_at: f64,
    /// Whether a `certain` pair is linked without a steward.
    pub auto_link: bool,
    /// The keys a pair must share one of to be scored.
    pub blocking_keys: Vec<BlockingKey>,
    /// The weights that replace the scorer's defaults, by field
    /// (`[matching.fields]`).
    pub fields: BTreeMap<Field, Weights>,
}

impl Default for Matching {
    fn default() -> Self {
        Self {
            link_at: 0.95,
            review_at: 0.80,
            possible_at: 0.60,
            auto_link: false,
            blocking_keys: vec![
                BlockingKey::BirthYearFamilyPhonetic,
                BlockingKey::PostalCodeBirthDate,
                BlockingKey::SharedIdentifier,
            ],
            fields: BTreeMap::new(),
        }
    }
}

impl Matching {
    /// Checks the section and returns it as resolved.
    ///
    /// # Errors
    /// Returns [`Error::Value`] for thresholds that are not
    /// `0 < possible_at <= review_at <= link_at <= 1`, no blocking key or one
    /// named twice, and weights outside `0 < u < m < 1`.
    pub fn resolve(&self) -> Result<Self, Error> {
        let ordered = 0.0 < self.possible_at
            && self.possible_at <= self.review_at
            && self.review_at <= self.link_at
            && self.link_at <= 1.0;
        if !ordered {
            return Err(value(
                "matching.link_at",
                "the thresholds hold 0 < possible_at <= review_at <= link_at <= 1",
            ));
        }
        let mut keys = self.blocking_keys.clone();
        keys.sort_unstable();
        keys.dedup();
        if keys.is_empty() || keys.len() != self.blocking_keys.len() {
            return Err(value(
                "matching.blocking_keys",
                "at least one key, each named once",
            ));
        }
        for (field, weights) in &self.fields {
            // NOTE: no specification governs this: our own design; with u < m
            // agreement adds weight, log2(m / u) > 0, and both logs are finite.
            if !(0.0 < weights.u && weights.u < weights.m && weights.m < 1.0) {
                return Err(Error::Value {
                    key: format!("matching.fields.{}", field.key()),
                    rule: "the probabilities hold 0 < u < m < 1",
                });
            }
        }
        Ok(self.clone())
    }
}

/// Returns an [`Error::Value`] under `key`.
fn value(key: &str, rule: &'static str) -> Error {
    Error::Value {
        key: key.to_owned(),
        rule,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{BlockingKey, Field, Matching, Weights};
    use crate::config::error::Error;

    #[test]
    fn the_defaults_resolve_and_keep_automatic_linking_off() {
        let matching = Matching::default().resolve().expect("the defaults resolve");
        assert!(!matching.auto_link);
        assert_eq!(3, matching.blocking_keys.len());
    }

    #[test]
    fn thresholds_out_of_order_or_outside_the_unit_interval_are_refused() {
        for (link, review, possible) in [
            (0.80, 0.95, 0.60),
            (0.95, 0.60, 0.80),
            (1.10, 0.80, 0.60),
            (0.95, 0.80, 0.0),
        ] {
            let matching = Matching {
                link_at: link,
                review_at: review,
                possible_at: possible,
                ..Matching::default()
            };
            assert!(
                matches!(matching.resolve(), Err(Error::Value { .. })),
                "{link} {review} {possible}"
            );
        }
    }

    #[test]
    fn no_blocking_key_and_a_key_named_twice_are_refused() {
        for keys in [
            Vec::new(),
            vec![BlockingKey::SharedIdentifier, BlockingKey::SharedIdentifier],
        ] {
            let matching = Matching {
                blocking_keys: keys,
                ..Matching::default()
            };
            assert!(matches!(matching.resolve(), Err(Error::Value { .. })));
        }
    }

    #[test]
    fn every_field_key_is_the_name_the_table_reads() {
        for field in [
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
        ] {
            let text = format!("[fields.{}]\nm = 0.9\nu = 0.1\n", field.key());
            let read: Matching = toml::from_str(&text).expect("the key reads");
            assert!(read.fields.contains_key(&field), "{}", field.key());
        }
    }

    #[test]
    fn weights_outside_zero_u_m_one_are_refused_and_named_by_field() {
        for (m, u) in [(0.5, 0.9), (1.0, 0.1), (0.9, 0.0), (0.5, 0.5)] {
            let matching = Matching {
                fields: BTreeMap::from([(Field::FamilyName, Weights { m, u })]),
                ..Matching::default()
            };
            let error = matching.resolve().expect_err("refused");
            assert!(
                matches!(&error, Error::Value { key, .. } if key == "matching.fields.family_name"),
                "{m} {u}: {error:?}"
            );
        }
    }
}
