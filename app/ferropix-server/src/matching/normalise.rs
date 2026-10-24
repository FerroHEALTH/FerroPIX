// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The forms the scorer compares values in.
//!
//! A name or an address is folded to its base letters, lowercased, with
//! every run of other characters one space; a postal code is uppercased
//! without spaces; a telephone number keeps its digits; an email address is
//! trimmed and lowercased. No specification governs the forms: our own
//! design (`docs/architecture.md` §5.2).

use unicode_normalization::UnicodeNormalization as _;
use unicode_normalization::char::is_combining_mark;

/// The digits a telephone number is compared on: a Dutch subscriber number
/// after its trunk or country prefix.
const TELEPHONE_DIGITS: usize = 9;

/// Returns `text` folded to its base letters and digits, lowercased, each
/// run of other characters one space, or `None` when nothing is left.
#[must_use]
pub fn text(text: &str) -> Option<String> {
    let mut folded = String::with_capacity(text.len());
    let mut gap = false;
    for character in text.nfkd().filter(|c| !is_combining_mark(*c)) {
        if character.is_alphanumeric() {
            if gap && !folded.is_empty() {
                folded.push(' ');
            }
            gap = false;
            folded.extend(character.to_lowercase());
        } else {
            gap = true;
        }
    }
    (!folded.is_empty()).then_some(folded)
}

/// Returns the words of `text` as [`text`] folds it.
#[must_use]
pub fn words(text: &str) -> Vec<String> {
    self::text(text)
        .map(|folded| folded.split(' ').map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Returns `code` uppercased without whitespace, or `None` when empty.
#[must_use]
pub fn postal_code(code: &str) -> Option<String> {
    let folded: String = code
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect();
    (!folded.is_empty()).then_some(folded)
}

/// Returns the last nine digits of `number`, or `None` when it holds fewer.
///
/// `+31 6 1234 5678`, `0031612345678` and `06-12345678` all read
/// `612345678`, so the international and the national form agree.
#[must_use]
pub fn telephone(number: &str) -> Option<String> {
    let digits: Vec<char> = number.chars().filter(char::is_ascii_digit).collect();
    let skip = digits.len().checked_sub(TELEPHONE_DIGITS)?;
    Some(digits.into_iter().skip(skip).collect())
}

/// Returns `address` trimmed and lowercased, or `None` when empty.
#[must_use]
pub fn email(address: &str) -> Option<String> {
    let folded = address.trim().to_lowercase();
    (!folded.is_empty()).then_some(folded)
}

#[cfg(test)]
mod tests {
    use super::{email, postal_code, telephone, text, words};

    #[test]
    fn a_name_folds_to_its_base_letters_lowercased_with_one_space_per_gap() {
        assert_eq!(
            Some(String::from("van der proefsma")),
            text(" Van der  Próefsma ")
        );
        assert_eq!(Some(String::from("elin")), text("Ëlin"));
        assert_eq!(Some(String::from("ver zinga")), text("Ver'Zinga"));
        assert_eq!(None, text(" -' "));
        assert_eq!(vec!["de", "testerink"], words("de Testerink"));
    }

    #[test]
    fn a_postal_code_is_uppercased_without_spaces() {
        assert_eq!(Some(String::from("1234AB")), postal_code("1234 ab"));
        assert_eq!(None, postal_code("  "));
    }

    #[test]
    fn the_national_and_the_international_telephone_forms_agree() {
        let national = telephone("06-12345678");
        assert_eq!(Some(String::from("612345678")), national);
        assert_eq!(national, telephone("+31 6 1234 5678"));
        assert_eq!(national, telephone("0031612345678"));
        assert_eq!(None, telephone("12345"));
    }

    #[test]
    fn an_email_address_is_trimmed_and_lowercased() {
        assert_eq!(
            Some(String::from("ada@example.org")),
            email(" Ada@Example.org ")
        );
        assert_eq!(None, email(" "));
    }
}
