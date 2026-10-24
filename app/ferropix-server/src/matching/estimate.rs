// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `u` of a field, estimated from how often each of its values occurs.
//!
//! `u` is the probability that a field agrees for two records picked at
//! random (`docs/architecture.md` §5.2). Over the records that state the
//! field, two random ones agree with probability `Σ (n_v / N)²`, the sum
//! over each value `v` held by `n_v` of the `N` records. `ferropix matching
//! estimate` reads the counts and nothing else. No specification governs the
//! estimate: our own design.

use crate::config::section::matching::Field;
use crate::matching::subject::Subject;

/// The fewest records a field must be stated by for its estimate to be
/// given.
pub const MIN_RECORDS: u64 = 100;

/// Returns the value `field` of `subject` is counted under, in its
/// normalised form, or `None` when the subject does not state it.
#[must_use]
pub fn value(field: Field, subject: &Subject) -> Option<String> {
    match field {
        Field::FamilyName => subject.family().map(str::to_owned),
        Field::GivenNames => subject
            .names
            .iter()
            .find_map(|name| name.given.first().cloned()),
        Field::BirthDate => subject
            .birth
            .and_then(crate::matching::subject::Birth::digits),
        Field::Gender => subject.gender.clone(),
        Field::PostalCode => subject.postal_codes.first().cloned(),
        Field::Address => subject
            .addresses
            .first()
            .map(|words| words.iter().cloned().collect::<Vec<_>>().join(" ")),
        Field::Telephone => subject.telephones.first().cloned(),
        Field::Email => subject.emails.first().cloned(),
        Field::MothersMaidenName => subject.mothers_maiden_name.clone(),
        Field::LocalIdentifier => None,
    }
}

/// Returns `Σ (n_v / N)²` over `counts`, or `None` when they sum to fewer
/// than [`MIN_RECORDS`].
#[must_use]
pub fn u_of(counts: impl IntoIterator<Item = u64> + Clone) -> Option<f64> {
    let total: u64 = counts.clone().into_iter().fold(0, u64::saturating_add);
    if total < MIN_RECORDS {
        return None;
    }
    let whole = to_f64(total)?;
    let sum = counts
        .into_iter()
        .map(|count| to_f64(count).map(|count| (count / whole).powi(2)))
        .sum::<Option<f64>>()?;
    Some(sum)
}

/// Returns `count` as an `f64`, or `None` past `u32::MAX` records.
fn to_f64(count: u64) -> Option<f64> {
    u32::try_from(count).ok().map(f64::from)
}

#[cfg(test)]
mod tests {
    use super::{MIN_RECORDS, u_of};

    #[test]
    fn u_is_the_sum_of_the_squared_shares() {
        let u = u_of([50_u64, 50]).unwrap();
        assert!((u - 0.5).abs() < 1e-12, "{u}");
        let u = u_of(std::iter::repeat_n(1_u64, 1000)).unwrap();
        assert!((u - 0.001).abs() < 1e-12, "{u}");
    }

    #[test]
    fn too_few_records_give_no_estimate() {
        assert_eq!(None, u_of([MIN_RECORDS - 1]));
        assert!(u_of([MIN_RECORDS]).is_some());
    }
}
