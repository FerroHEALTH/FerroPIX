// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! `GET {base}/operator/matching`: the `[matching]` section the feed and the
//! scorer run with, every field's weights included, the configured ones
//! marked.

use axum::response::Response;
use ferropix_operator_api::matching::{FieldWeights, Matching};

use crate::config::section::matching::BlockingKey;
use crate::matching::scorer::{FIELDS, default_weights};
use crate::operator::{Operator, wire};

/// Answers the matching configuration in effect.
#[must_use]
pub fn answer(operator: &Operator) -> Response {
    wire::answer(&document(&operator.matching))
}

/// Returns the document of `matching`.
#[must_use]
pub fn document(matching: &crate::config::section::matching::Matching) -> Matching {
    Matching {
        link_at: matching.link_at,
        review_at: matching.review_at,
        possible_at: matching.possible_at,
        auto_link: matching.auto_link,
        blocking_keys: matching
            .blocking_keys
            .iter()
            .map(|key| blocking_key(*key).to_owned())
            .collect(),
        fields: FIELDS
            .into_iter()
            .map(|field| {
                let configured = matching.fields.get(&field).copied();
                let weights = configured.unwrap_or_else(|| default_weights(field));
                FieldWeights {
                    field: field.key().to_owned(),
                    m: weights.m,
                    u: weights.u,
                    configured: configured.is_some(),
                }
            })
            .collect(),
    }
}

/// Returns the name `[matching] blocking_keys` gives `key`.
const fn blocking_key(key: BlockingKey) -> &'static str {
    match key {
        BlockingKey::BirthYearFamilyPhonetic => "birth_year_family_phonetic",
        BlockingKey::PostalCodeBirthDate => "postal_code_birth_date",
        BlockingKey::SharedIdentifier => "shared_identifier",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::document;
    use crate::config::section::matching::{BlockingKey, Field, Matching, Weights};

    #[test]
    fn every_field_is_listed_with_its_weights_and_the_configured_one_is_marked() {
        let matching = Matching {
            fields: BTreeMap::from([(Field::Gender, Weights { m: 0.9, u: 0.4 })]),
            blocking_keys: vec![BlockingKey::SharedIdentifier],
            ..Matching::default()
        };
        let read = document(&matching);
        assert_eq!(10, read.fields.len());
        let gender = read
            .fields
            .iter()
            .find(|field| field.field == "gender")
            .expect("gender");
        assert!(gender.configured);
        assert!((gender.u - 0.4).abs() < f64::EPSILON);
        assert!(
            read.fields
                .iter()
                .filter(|field| field.field != "gender")
                .all(|field| !field.configured)
        );
        assert_eq!(vec![String::from("shared_identifier")], read.blocking_keys);
        let text = toml::to_string(&matching).expect("the section writes");
        assert!(text.contains("shared_identifier"), "{text}");
    }
}
