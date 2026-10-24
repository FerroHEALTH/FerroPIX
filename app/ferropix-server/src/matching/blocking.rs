// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The blocking keys: a pair is scored only when it shares one.
//!
//! `(birth year, the phonetic key of the family name)` and `(postal code,
//! birth date)` are text keys the store keeps as indexed columns of the
//! version, and an identifier shared across domains is found through the
//! identifiers each version carries (`docs/architecture.md` §5.2). The
//! phonetic key is American Soundex over the last word of the family name,
//! so a Dutch prefix (`van`, `de`, `van der`) does not decide the block. No
//! specification governs the keys: our own design.

use crate::config::section::matching::BlockingKey;
use crate::matching::subject::Subject;

/// The length of a Soundex code: a letter and three digits.
const SOUNDEX_LEN: usize = 4;

/// Returns the text key `key` gives `subject`, or `None` when the subject
/// lacks a part of it or the key is no text key.
#[must_use]
pub fn text_key(key: BlockingKey, subject: &Subject) -> Option<String> {
    match key {
        BlockingKey::BirthYearFamilyPhonetic => {
            let year = subject.birth?.year;
            let last = subject.family()?.rsplit(' ').next()?;
            Some(format!("{year:04}|{}", soundex(last)?))
        }
        BlockingKey::PostalCodeBirthDate => {
            let postal = subject.postal_codes.first()?;
            let date = subject.birth?.digits()?;
            Some(format!("{postal}|{date}"))
        }
        BlockingKey::SharedIdentifier => None,
    }
}

/// Returns the American Soundex code of `word`'s ASCII letters, or `None`
/// when it holds none.
///
/// The first letter is kept; each later consonant is coded `b f p v` 1,
/// `c g j k q s x z` 2, `d t` 3, `l` 4, `m n` 5, `r` 6; a code equal to the
/// one before it, or separated from it only by `h` or `w`, is dropped;
/// vowels are dropped and separate equal codes; the result is padded with
/// `0` to four characters (the rules the U.S. National Archives publish,
/// <https://www.archives.gov/research/census/soundex>).
#[must_use]
pub fn soundex(word: &str) -> Option<String> {
    let mut letters = word
        .chars()
        .filter(char::is_ascii_alphabetic)
        .map(|c| c.to_ascii_lowercase());
    let first = letters.next()?;
    let mut code = String::with_capacity(SOUNDEX_LEN);
    code.push(first.to_ascii_uppercase());
    let mut previous = digit(first);
    for letter in letters {
        if code.len() == SOUNDEX_LEN {
            break;
        }
        match digit(letter) {
            Some(d) if Some(d) != previous => {
                code.push(d);
                previous = Some(d);
            }
            Some(_) => {}
            None if matches!(letter, 'h' | 'w') => {}
            None => previous = None,
        }
    }
    while code.len() < SOUNDEX_LEN {
        code.push('0');
    }
    Some(code)
}

/// The Soundex digit of a consonant, `None` for a vowel, `h`, `w` and `y`.
const fn digit(letter: char) -> Option<char> {
    match letter {
        'b' | 'f' | 'p' | 'v' => Some('1'),
        'c' | 'g' | 'j' | 'k' | 'q' | 's' | 'x' | 'z' => Some('2'),
        'd' | 't' => Some('3'),
        'l' => Some('4'),
        'm' | 'n' => Some('5'),
        'r' => Some('6'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use ferropix_testkit::synthetic::FAMILY_NAMES;

    use super::{soundex, text_key};
    use crate::config::section::matching::BlockingKey;
    use crate::matching::subject::{Birth, Name, Subject};

    #[test]
    fn soundex_follows_the_archives_rules() {
        for (word, code) in [
            ("robert", "R163"),
            ("rupert", "R163"),
            ("rubin", "R150"),
            ("ashcraft", "A261"),
            ("tymczak", "T522"),
            ("pfister", "P236"),
            ("honeyman", "H555"),
        ] {
            assert_eq!(Some(String::from(code)), soundex(word), "{word}");
        }
        assert_eq!(None, soundex("123"));
    }

    #[test]
    fn the_text_keys_read_the_year_the_last_family_word_the_postal_code_and_the_date() {
        let family = FAMILY_NAMES[0].to_lowercase();
        let subject = Subject {
            names: vec![Name {
                family: Some(format!("van der {family}")),
                given: Vec::new(),
            }],
            birth: Some(Birth {
                year: 1958,
                day: Some((3, 14)),
            }),
            postal_codes: vec![String::from("1234AB")],
            ..Subject::default()
        };
        let phonetic = soundex(&family).unwrap();
        assert_eq!(
            Some(format!("1958|{phonetic}")),
            text_key(BlockingKey::BirthYearFamilyPhonetic, &subject)
        );
        assert_eq!(
            Some(String::from("1234AB|19580314")),
            text_key(BlockingKey::PostalCodeBirthDate, &subject)
        );
        assert_eq!(None, text_key(BlockingKey::SharedIdentifier, &subject));
        assert_eq!(
            None,
            text_key(BlockingKey::PostalCodeBirthDate, &Subject::default())
        );
    }
}
