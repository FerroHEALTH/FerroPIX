// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! An identifier value in the form its domain's rule gives it, before it is
//! sealed or found by its HMAC.
//!
//! The deterministic match links on "the same normalised value in the same
//! authoritative domain" (`docs/architecture.md` §5.1), so two feeds that
//! write one BSN with and without its separators must seal and find one
//! value. A BSN keeps its digits, takes back a leading zero dropped from
//! the nine, and must pass the eleven-proof; every other domain's value is
//! trimmed of surrounding whitespace and keeps its case, since FHIR R4
//! compares an `Identifier.value` as written. No specification governs the
//! rules: our own design.

use nl_generic_functions::identification::is_bsn_system;

use crate::store::seal::IdentifierValue;

/// The digits of a BSN.
const BSN_DIGITS: usize = 9;

/// The weight of each digit of a BSN in the eleven-proof, the last one
/// subtracted.
const ELEVEN_PROOF: [u32; BSN_DIGITS] = [9, 8, 7, 6, 5, 4, 3, 2, 1];

/// The rule a domain's identifier values follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// The BSN: nine digits passing the eleven-proof.
    Bsn,
    /// Any other domain: the value trimmed, its case kept.
    Trimmed,
}

impl Rule {
    /// Returns the rule of the domain `system` names.
    #[must_use]
    pub fn of(system: &str) -> Self {
        if is_bsn_system(system) {
            Self::Bsn
        } else {
            Self::Trimmed
        }
    }

    /// Returns `raw` in this rule's form.
    ///
    /// # Errors
    /// Returns [`Refused`] for a value the rule does not admit: an empty
    /// one, or a BSN that is not nine digits passing the eleven-proof.
    pub fn normalise(self, raw: &str) -> Result<IdentifierValue, Refused> {
        match self {
            Self::Trimmed => IdentifierValue::new(raw.trim()).ok_or(Refused::Empty),
            Self::Bsn => bsn(raw),
        }
    }
}

/// Returns the BSN `raw` holds: its digits once spaces, dots and dashes are
/// dropped, with a leading zero restored to an eight-digit number.
fn bsn(raw: &str) -> Result<IdentifierValue, Refused> {
    let kept: String = raw
        .chars()
        .filter(|c| !(c.is_whitespace() || *c == '.' || *c == '-'))
        .collect();
    if kept.is_empty() {
        return Err(Refused::Empty);
    }
    if !kept.chars().all(|c| c.is_ascii_digit()) {
        return Err(Refused::NotABsn);
    }
    let digits = match kept.len() {
        8 => format!("0{kept}"),
        BSN_DIGITS => kept,
        _ => return Err(Refused::NotABsn),
    };
    let mut sum: u32 = 0;
    let mut check: u32 = 0;
    for (index, (digit, weight)) in digits.chars().zip(ELEVEN_PROOF).enumerate() {
        let value = digit.to_digit(10).ok_or(Refused::NotABsn)?;
        if index + 1 == BSN_DIGITS {
            check = value;
        } else {
            sum = sum.saturating_add(value.saturating_mul(weight));
        }
    }
    // NOTE: no specification FerroPIX vendors defines the proof; the BSN's
    // own: the weighted sum of the first eight digits equals the ninth mod 11.
    if sum % 11 != check {
        return Err(Refused::NotABsn);
    }
    IdentifierValue::new(digits).ok_or(Refused::Empty)
}

/// Why a value is no identifier of its domain.
///
/// No variant carries the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Refused {
    /// The value is empty once trimmed.
    #[error("the identifier value is empty")]
    Empty,
    /// The value is no BSN: not nine digits, or failing the eleven-proof.
    #[error("the identifier value is no BSN of nine digits passing the eleven-proof")]
    NotABsn,
}

#[cfg(test)]
mod tests {
    use ferropix_testkit::synthetic::bsn;

    use super::{Refused, Rule};

    const BSN: &str = "http://fhir.nl/fhir/NamingSystem/bsn";

    #[test]
    fn a_bsn_with_separators_reads_as_its_nine_digits() {
        let digits = bsn(0).unwrap().digits();
        let part = |from: usize, to: usize| -> String {
            digits.chars().skip(from).take(to - from).collect()
        };
        let spaced = format!("{} {}.{}", part(0, 3), part(3, 6), part(6, 9));
        let value = Rule::of(BSN).normalise(&spaced).unwrap();
        assert_eq!(digits, value.expose());
    }

    #[test]
    fn a_bsn_failing_the_proof_or_of_the_wrong_length_is_refused() {
        let digits = bsn(0).unwrap().digits();
        let last = digits.chars().last().unwrap().to_digit(10).unwrap();
        let head: String = digits.chars().take(8).collect();
        let broken = format!("{head}{}", (last + 1) % 10);
        assert_eq!(
            Err(Refused::NotABsn),
            Rule::of(BSN).normalise(&broken).map(|_| ())
        );
        assert_eq!(
            Err(Refused::NotABsn),
            Rule::of(BSN).normalise("12345").map(|_| ())
        );
        assert_eq!(
            Err(Refused::NotABsn),
            Rule::of(BSN).normalise("99999000x").map(|_| ())
        );
        assert_eq!(
            Err(Refused::Empty),
            Rule::of(BSN).normalise(" . ").map(|_| ())
        );
    }

    #[test]
    fn an_eight_digit_bsn_takes_back_its_leading_zero() {
        let eight = "11222333";
        let value = Rule::Bsn.normalise(eight);
        let proof_holds = {
            let digits = format!("0{eight}");
            let sum: u32 = digits
                .chars()
                .take(8)
                .zip([9, 8, 7, 6, 5, 4, 3, 2])
                .map(|(c, w)| c.to_digit(10).unwrap() * w)
                .sum();
            sum % 11 == digits.chars().last().unwrap().to_digit(10).unwrap()
        };
        assert_eq!(proof_holds, value.is_ok());
        if let Ok(value) = value {
            assert_eq!(9, value.expose().len());
        }
    }

    #[test]
    fn any_other_domain_trims_and_keeps_the_case() {
        let value = Rule::of("urn:oid:2.999.1.1")
            .normalise("  H-100001x ")
            .unwrap();
        assert_eq!("H-100001x", value.expose());
        assert_eq!(
            Err(Refused::Empty),
            Rule::Trimmed.normalise("   ").map(|_| ())
        );
    }
}
