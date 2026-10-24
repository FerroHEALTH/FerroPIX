// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The synthetic values every fixture draws from: BSNs from one reserved
//! range, identifier domains under the example OID arc, and a closed set of
//! invented names.
//!
//! A BSN has nine digits and passes the eleven-proof: the first eight digits
//! weighted 9 down to 2, minus the ninth, is a multiple of 11 (the test
//! [`is_eleven_proof`] applies). FerroPIX reserves the range
//! [`SYNTHETIC_BSN_FIRST`] to [`SYNTHETIC_BSN_LAST`], the numbers that
//! start with `99999`, for its fixtures; no specification FerroPIX vendors
//! reserves a range, so the choice is our own design. The range holds 909
//! numbers that pass the proof, and [`bsn`] hands them out by index, so a
//! suite never types one.

use std::fmt;

/// The first number of the synthetic BSN range.
pub const SYNTHETIC_BSN_FIRST: u32 = 999_990_000;

/// The last number of the synthetic BSN range.
pub const SYNTHETIC_BSN_LAST: u32 = 999_999_999;

/// The identifier domains a fixture may name, each an OID under `2.999`,
/// the arc ITU-T X.660 sets aside for examples, so none is an assigning
/// authority anywhere.
pub const SYNTHETIC_DOMAINS: [&str; 3] = [HOSPITAL_DOMAIN, LABORATORY_DOMAIN, PRACTICE_DOMAIN];

/// The synthetic hospital's medical record numbers.
pub const HOSPITAL_DOMAIN: &str = "urn:oid:2.999.1.1";

/// The synthetic laboratory's patient numbers.
pub const LABORATORY_DOMAIN: &str = "urn:oid:2.999.1.2";

/// The synthetic general practice's patient numbers.
pub const PRACTICE_DOMAIN: &str = "urn:oid:2.999.1.3";

/// The family names a fixture may carry, invented for the tests.
pub const FAMILY_NAMES: [&str; 8] = [
    "Proefsma",
    "Testerink",
    "Voorbeeldt",
    "Fictiefhuis",
    "Verzinga",
    "Steekproef",
    "Monsterman",
    "Probeerda",
];

/// The given names a fixture may carry, invented for the tests.
pub const GIVEN_NAMES: [&str; 8] = [
    "Ada", "Bram", "Cato", "Daan", "Elin", "Floor", "Gijs", "Hanna",
];

/// The weight of each digit of a BSN in the eleven-proof.
const ELEVEN_PROOF_WEIGHTS: [i64; 9] = [9, 8, 7, 6, 5, 4, 3, 2, -1];

/// The synthetic range holds fewer BSNs than the index asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the synthetic BSN range holds no number at index {index}")]
pub struct RangeExhausted {
    /// The index that was asked for.
    pub index: usize,
}

/// A BSN from the synthetic range, valid under the eleven-proof.
///
/// Its `Debug` and `Display` print the number: every value of this type is
/// synthetic by construction.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SyntheticBsn(u32);

impl SyntheticBsn {
    /// Returns the nine digits of the number.
    #[must_use]
    pub fn digits(self) -> String {
        format!("{:09}", self.0)
    }
}

impl fmt::Debug for SyntheticBsn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SyntheticBsn({:09})", self.0)
    }
}

impl fmt::Display for SyntheticBsn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:09}", self.0)
    }
}

/// Returns the synthetic BSN at `index`, counting the numbers of the range
/// that pass the eleven-proof in ascending order.
///
/// # Examples
///
/// ```
/// use ferropix_testkit::synthetic::{bsn, is_eleven_proof, is_synthetic_bsn};
///
/// let first = bsn(0)?.digits();
/// assert!(is_eleven_proof(&first));
/// assert!(is_synthetic_bsn(&first));
/// # Ok::<(), ferropix_testkit::synthetic::RangeExhausted>(())
/// ```
///
/// # Errors
///
/// Returns [`RangeExhausted`] when `index` is 909 or more.
pub fn bsn(index: usize) -> Result<SyntheticBsn, RangeExhausted> {
    (SYNTHETIC_BSN_FIRST..=SYNTHETIC_BSN_LAST)
        .filter(|number| is_eleven_proof(&format!("{number:09}")))
        .nth(index)
        .map(SyntheticBsn)
        .ok_or(RangeExhausted { index })
}

/// Returns whether `value` is nine ASCII digits that pass the eleven-proof.
#[must_use]
pub fn is_eleven_proof(value: &str) -> bool {
    if value.len() != 9 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let sum: i64 = value
        .bytes()
        .zip(ELEVEN_PROOF_WEIGHTS)
        .map(|(byte, weight)| weight * i64::from(byte - b'0'))
        .sum();
    sum.rem_euclid(11) == 0
}

/// Returns whether `value` is a BSN of the synthetic range: nine digits,
/// inside the range, passing the eleven-proof.
#[must_use]
pub fn is_synthetic_bsn(value: &str) -> bool {
    is_eleven_proof(value)
        && value
            .parse::<u32>()
            .is_ok_and(|number| (SYNTHETIC_BSN_FIRST..=SYNTHETIC_BSN_LAST).contains(&number))
}

#[cfg(test)]
mod tests {
    use super::{RangeExhausted, bsn, is_eleven_proof, is_synthetic_bsn};

    #[test]
    fn the_first_synthetic_bsns_are_the_lowest_that_pass_the_proof() {
        let first: Vec<String> = (0..3).map(|i| bsn(i).unwrap().digits()).collect();
        assert_eq!(vec!["999990007", "999990019", "999990020"], first);
    }

    #[test]
    fn the_range_holds_909_numbers() {
        assert!(bsn(908).is_ok(), "the last index is 908");
        assert_eq!(Err(RangeExhausted { index: 909 }), bsn(909));
    }

    #[test]
    fn the_proof_refuses_a_changed_check_digit() {
        assert!(is_eleven_proof("999990007"), "the proof holds");
        assert!(!is_eleven_proof("999990008"), "one digit off breaks it");
    }

    #[test]
    fn the_proof_refuses_anything_but_nine_digits() {
        for value in ["", "99999000", "9999900070", "99999000x", " 99999000"] {
            assert!(!is_eleven_proof(value), "{value:?} is not nine digits");
        }
    }

    #[test]
    fn a_number_outside_the_range_is_not_synthetic_even_when_valid() {
        let outside = (100_000_000_u32..=100_000_099)
            .map(|number| number.to_string())
            .find(|digits| is_eleven_proof(digits))
            .unwrap();
        assert!(!is_synthetic_bsn(&outside), "outside the range");
    }
}
