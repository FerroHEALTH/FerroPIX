// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The comparator of each field, as the table of `docs/architecture.md` §5.2
//! names it.
//!
//! Each comparator reads both subjects and answers one [`Comparison`]: the
//! agreement, the comparator's reading where it has one (a Jaro-Winkler
//! similarity, a token overlap), and the two values it compared. Where a
//! field holds several values (names, addresses, telecom), the best pair
//! counts. Every comparator is symmetric. No specification governs the
//! comparators: our own design.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::config::section::matching::Field;
use crate::matching::subject::{Birth, IdentifierKey, Subject};

/// The Jaro-Winkler similarity at or above which a name agrees whole.
pub const NAME_AGREES: f64 = 0.92;

/// The Jaro-Winkler similarity at or above which a name agrees half.
pub const NAME_PARTIAL: f64 = 0.85;

/// The share of the smaller address's words the other must hold to agree.
pub const ADDRESS_AGREES: f64 = 0.5;

/// The characters of a postal code whose agreement counts half.
pub const POSTAL_PREFIX: usize = 4;

/// How a field compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Agreement {
    /// The field is absent on one side or both: it adds no weight.
    Absent,
    /// The field differs: it adds the disagreement weight.
    Disagree,
    /// The field agrees in part: it adds half the agreement weight.
    Partial,
    /// The field agrees: it adds the agreement weight.
    Agree,
}

/// The comparator a field is read with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Comparator {
    /// Jaro-Winkler similarity over the normalised text.
    JaroWinkler,
    /// Equality, with the partial credit the field names.
    Exact,
    /// Equality, then the first characters.
    Prefix,
    /// The share of shared words.
    TokenOverlap,
    /// Equality of the identifiers' HMACs within a shared domain.
    Identifier,
}

/// What one comparator found.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// The comparator.
    pub comparator: Comparator,
    /// How the field compared.
    pub agreement: Agreement,
    /// The comparator's reading, where it has one.
    pub reading: Option<f64>,
    /// The values compared, in the normalised form, when the field holds
    /// a value the evidence may show.
    pub values: Option<(String, String)>,
}

impl Comparison {
    /// Returns a comparison of a field absent on one side or both.
    const fn absent(comparator: Comparator) -> Self {
        Self {
            comparator,
            agreement: Agreement::Absent,
            reading: None,
            values: None,
        }
    }
}

/// Compares `field` of `a` and `b`.
#[must_use]
pub fn compare(field: Field, a: &Subject, b: &Subject) -> Comparison {
    match field {
        Field::FamilyName => family(a, b),
        Field::GivenNames => given(a, b),
        Field::BirthDate => birth(a.birth, b.birth),
        Field::Gender => exact(one(a.gender.as_ref()), one(b.gender.as_ref())),
        Field::PostalCode => postal(&a.postal_codes, &b.postal_codes),
        Field::Address => address(&a.addresses, &b.addresses),
        Field::Telephone => exact(&a.telephones, &b.telephones),
        Field::Email => exact(&a.emails, &b.emails),
        Field::MothersMaidenName => jaro_winkler(
            one(a.mothers_maiden_name.as_ref()),
            one(b.mothers_maiden_name.as_ref()),
            false,
        ),
        Field::LocalIdentifier => identifiers(a, b),
    }
}

/// The family names, Jaro-Winkler with half credit between 0.85 and 0.92.
fn family(a: &Subject, b: &Subject) -> Comparison {
    let names = |subject: &Subject| -> Vec<String> {
        subject
            .names
            .iter()
            .filter_map(|name| name.family.clone())
            .collect()
    };
    jaro_winkler(&names(a), &names(b), true)
}

/// The best pair of `a` and `b` by Jaro-Winkler; with `partial`, a
/// similarity between [`NAME_PARTIAL`] and [`NAME_AGREES`] agrees half.
fn jaro_winkler(a: &[String], b: &[String], partial: bool) -> Comparison {
    let best = best_pair(a, b, |x, y| strsim::jaro_winkler(x, y));
    let Some((similarity, x, y)) = best else {
        return Comparison::absent(Comparator::JaroWinkler);
    };
    let agreement = if similarity >= NAME_AGREES {
        Agreement::Agree
    } else if partial && similarity >= NAME_PARTIAL {
        Agreement::Partial
    } else {
        Agreement::Disagree
    };
    Comparison {
        comparator: Comparator::JaroWinkler,
        agreement,
        reading: Some(similarity),
        values: Some((x.clone(), y.clone())),
    }
}

/// The given names: the first given name by Jaro-Winkler, or any given name
/// equal; half credit for a similarity between 0.85 and 0.92 or for agreeing
/// initials.
fn given(a: &Subject, b: &Subject) -> Comparison {
    let first = |subject: &Subject| -> Vec<String> {
        subject
            .names
            .iter()
            .filter_map(|name| name.given.first().cloned())
            .collect()
    };
    let all = |subject: &Subject| -> BTreeSet<String> {
        subject
            .names
            .iter()
            .flat_map(|name| name.given.iter().cloned())
            .collect()
    };
    let mut comparison = jaro_winkler(&first(a), &first(b), true);
    if comparison.agreement == Agreement::Absent {
        return comparison;
    }
    let (all_a, all_b) = (all(a), all(b));
    // NOTE: no specification governs this: our own design; a one-letter given
    // name is an initial, which only the initials rule may credit.
    if let Some(shared) = all_a
        .intersection(&all_b)
        .find(|name| name.chars().nth(1).is_some())
    {
        comparison.agreement = Agreement::Agree;
        comparison.values = Some((shared.clone(), shared.clone()));
        return comparison;
    }
    if comparison.agreement == Agreement::Disagree {
        let initial = |name: &String| name.chars().next();
        let initials_agree = comparison
            .values
            .as_ref()
            .is_some_and(|(x, y)| initial(x).is_some() && initial(x) == initial(y));
        if initials_agree {
            comparison.agreement = Agreement::Partial;
        }
    }
    comparison
}

/// The birth dates: equal to the day agrees; the day and month transposed,
/// or one digit off, agrees half; a date not stated to the day is absent.
fn birth(a: Option<Birth>, b: Option<Birth>) -> Comparison {
    let (Some(x), Some(y)) = (a.and_then(Birth::digits), b.and_then(Birth::digits)) else {
        return Comparison::absent(Comparator::Exact);
    };
    let transposed = matches!((a, b), (Some(a), Some(b))
        if a.year == b.year && a.day.map(|(m, d)| (d, m)) == b.day);
    let differing = x.chars().zip(y.chars()).filter(|(p, q)| p != q).count();
    let agreement = if x == y {
        Agreement::Agree
    } else if transposed || differing == 1 {
        Agreement::Partial
    } else {
        Agreement::Disagree
    };
    Comparison {
        comparator: Comparator::Exact,
        agreement,
        reading: None,
        values: Some((x, y)),
    }
}

/// Any value of `a` equal to any value of `b`.
fn exact(a: &[String], b: &[String]) -> Comparison {
    let (Some(first_a), Some(first_b)) = (a.first(), b.first()) else {
        return Comparison::absent(Comparator::Exact);
    };
    let shared = a.iter().find(|value| b.contains(value));
    let (agreement, values) = match shared {
        Some(value) => (Agreement::Agree, (value.clone(), value.clone())),
        None => (Agreement::Disagree, (first_a.clone(), first_b.clone())),
    };
    Comparison {
        comparator: Comparator::Exact,
        agreement,
        reading: None,
        values: Some(values),
    }
}

/// The postal codes: equal agrees, the first four characters equal agrees
/// half.
fn postal(a: &[String], b: &[String]) -> Comparison {
    let prefix = |code: &str| code.chars().take(POSTAL_PREFIX).collect::<String>();
    let mut comparison = exact(a, b);
    comparison.comparator = Comparator::Prefix;
    if comparison.agreement == Agreement::Disagree {
        let partial = a.iter().find_map(|x| {
            b.iter()
                .find(|y| x.chars().count() >= POSTAL_PREFIX && prefix(x) == prefix(y))
                .map(|y| (x.clone(), y.clone()))
        });
        if let Some(values) = partial {
            comparison.agreement = Agreement::Partial;
            comparison.values = Some(values);
        }
    }
    comparison
}

/// The addresses: the best pair by the share of the smaller address's words
/// the other holds, agreeing at [`ADDRESS_AGREES`] or more.
fn address(a: &[BTreeSet<String>], b: &[BTreeSet<String>]) -> Comparison {
    let best = best_pair(a, b, |x, y| {
        let shared = x.intersection(y).count();
        let smaller = x.len().min(y.len());
        ratio(shared, smaller)
    });
    let Some((overlap, x, y)) = best else {
        return Comparison::absent(Comparator::TokenOverlap);
    };
    let join = |words: &BTreeSet<String>| words.iter().cloned().collect::<Vec<_>>().join(" ");
    Comparison {
        comparator: Comparator::TokenOverlap,
        agreement: if overlap >= ADDRESS_AGREES {
            Agreement::Agree
        } else {
            Agreement::Disagree
        },
        reading: Some(overlap),
        values: Some((join(x), join(y))),
    }
}

/// The identifiers in `local` domains both subjects carry one in: the same
/// value in a shared domain agrees, different values disagree, no shared
/// domain is absent.
fn identifiers(a: &Subject, b: &Subject) -> Comparison {
    let domains = |subject: &Subject| -> BTreeSet<uuid::Uuid> {
        subject
            .local_identifiers
            .iter()
            .map(IdentifierKey::domain)
            .collect()
    };
    if domains(a).is_disjoint(&domains(b)) {
        return Comparison::absent(Comparator::Identifier);
    }
    let shared = !a.local_identifiers.is_disjoint(&b.local_identifiers);
    Comparison {
        comparator: Comparator::Identifier,
        agreement: if shared {
            Agreement::Agree
        } else {
            Agreement::Disagree
        },
        reading: None,
        values: None,
    }
}

/// The pair of `a` and `b` with the highest `measure`, the first such pair
/// in order on a tie.
fn best_pair<'v, T>(
    a: &'v [T],
    b: &'v [T],
    measure: impl Fn(&T, &T) -> f64,
) -> Option<(f64, &'v T, &'v T)> {
    let mut best: Option<(f64, &T, &T)> = None;
    for x in a {
        for y in b {
            let value = measure(x, y).max(measure(y, x));
            if best.is_none_or(|(top, _, _)| value > top) {
                best = Some((value, x, y));
            }
        }
    }
    best
}

/// Returns the one value `value` holds as a slice, or an empty one.
fn one(value: Option<&String>) -> &[String] {
    value.map(std::slice::from_ref).unwrap_or_default()
}

/// Returns `part / whole`, or `0` for an empty whole.
fn ratio(part: usize, whole: usize) -> f64 {
    match (u32::try_from(part), u32::try_from(whole)) {
        (Ok(part), Ok(whole)) if whole > 0 => f64::from(part) / f64::from(whole),
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES};

    use super::{Agreement, compare};
    use crate::config::section::matching::Field;
    use crate::matching::subject::{Birth, IdentifierKey, Name, Subject};

    fn named(family: &str, given: &[&str]) -> Subject {
        Subject {
            names: vec![Name {
                family: Some(family.to_lowercase()),
                given: given.iter().map(|g| g.to_lowercase()).collect(),
            }],
            ..Subject::default()
        }
    }

    fn born(year: u16, month: u8, day: u8) -> Subject {
        Subject {
            birth: Some(Birth {
                year,
                day: Some((month, day)),
            }),
            ..Subject::default()
        }
    }

    #[test]
    fn a_family_name_one_letter_off_agrees_and_a_different_one_disagrees() {
        let name = FAMILY_NAMES[0];
        let typo = format!("{name}a");
        let a = named(name, &[]);
        assert_eq!(
            Agreement::Agree,
            compare(Field::FamilyName, &a, &named(&typo, &[])).agreement
        );
        let other = named(FAMILY_NAMES[4], &[]);
        assert_eq!(
            Agreement::Disagree,
            compare(Field::FamilyName, &a, &other).agreement
        );
        assert_eq!(
            Agreement::Absent,
            compare(Field::FamilyName, &a, &Subject::default()).agreement
        );
    }

    #[test]
    fn a_family_name_between_the_two_similarities_agrees_half() {
        let a = named("proefsma", &[]);
        let b = named("proefsnaxx", &[]);
        let comparison = compare(Field::FamilyName, &a, &b);
        let reading = comparison.reading.unwrap();
        assert!((0.85..0.92).contains(&reading), "{reading}");
        assert_eq!(Agreement::Partial, comparison.agreement);
    }

    #[test]
    fn given_names_agree_on_any_shared_name_and_half_on_initials() {
        let family = FAMILY_NAMES[0];
        let (ada, bram) = (GIVEN_NAMES[0], GIVEN_NAMES[1]);
        let a = named(family, &[ada, bram]);
        let shared = named(family, &["Xeno", bram]);
        assert_eq!(
            Agreement::Agree,
            compare(Field::GivenNames, &a, &shared).agreement
        );
        let initial = named(family, &["A"]);
        assert_eq!(
            Agreement::Partial,
            compare(Field::GivenNames, &a, &initial).agreement
        );
        let other = named(family, &[GIVEN_NAMES[6]]);
        assert_eq!(
            Agreement::Disagree,
            compare(Field::GivenNames, &a, &other).agreement
        );
    }

    #[test]
    fn a_birth_date_transposed_or_one_digit_off_agrees_half() {
        let a = born(1958, 3, 4);
        assert_eq!(
            Agreement::Agree,
            compare(Field::BirthDate, &a, &born(1958, 3, 4)).agreement
        );
        assert_eq!(
            Agreement::Partial,
            compare(Field::BirthDate, &a, &born(1958, 4, 3)).agreement
        );
        assert_eq!(
            Agreement::Partial,
            compare(Field::BirthDate, &a, &born(1959, 3, 4)).agreement
        );
        assert_eq!(
            Agreement::Disagree,
            compare(Field::BirthDate, &a, &born(1971, 9, 12)).agreement
        );
        let year_only = Subject {
            birth: Some(Birth {
                year: 1958,
                day: None,
            }),
            ..Subject::default()
        };
        assert_eq!(
            Agreement::Absent,
            compare(Field::BirthDate, &a, &year_only).agreement
        );
    }

    #[test]
    fn a_postal_code_agrees_half_on_its_first_four_characters() {
        let at = |code: &str| Subject {
            postal_codes: vec![code.to_owned()],
            ..Subject::default()
        };
        let a = at("1234AB");
        assert_eq!(
            Agreement::Agree,
            compare(Field::PostalCode, &a, &at("1234AB")).agreement
        );
        assert_eq!(
            Agreement::Partial,
            compare(Field::PostalCode, &a, &at("1234ZZ")).agreement
        );
        assert_eq!(
            Agreement::Disagree,
            compare(Field::PostalCode, &a, &at("5678AB")).agreement
        );
    }

    #[test]
    fn an_address_agrees_when_half_the_smaller_one_s_words_are_shared() {
        let at = |words: &[&str]| Subject {
            addresses: vec![
                words
                    .iter()
                    .map(|w| (*w).to_owned())
                    .collect::<BTreeSet<_>>(),
            ],
            ..Subject::default()
        };
        let a = at(&["kade", "1", "voorbeeld"]);
        let b = at(&["kade", "1"]);
        let comparison = compare(Field::Address, &a, &b);
        assert_eq!(Agreement::Agree, comparison.agreement);
        assert_eq!(Some(1.0), comparison.reading);
        assert_eq!(
            Agreement::Disagree,
            compare(Field::Address, &a, &at(&["laan", "9"])).agreement
        );
    }

    #[test]
    fn a_local_identifier_compares_only_within_a_domain_both_carry() {
        let domain = uuid::Uuid::from_u128(1);
        let elsewhere = uuid::Uuid::from_u128(2);
        let carrying = |keys: &[IdentifierKey]| Subject {
            local_identifiers: keys.iter().copied().collect(),
            ..Subject::default()
        };
        let one = IdentifierKey::new(domain, [1; 32]);
        let two = IdentifierKey::new(domain, [2; 32]);
        let far = IdentifierKey::new(elsewhere, [1; 32]);
        let a = carrying(&[one]);
        assert_eq!(
            Agreement::Agree,
            compare(Field::LocalIdentifier, &a, &carrying(&[one])).agreement
        );
        assert_eq!(
            Agreement::Disagree,
            compare(Field::LocalIdentifier, &a, &carrying(&[two])).agreement
        );
        assert_eq!(
            Agreement::Absent,
            compare(Field::LocalIdentifier, &a, &carrying(&[far])).agreement
        );
    }

    #[test]
    fn an_absent_gender_adds_nothing_and_telecom_agrees_on_any_shared_value() {
        let gender = |g: &str| Subject {
            gender: Some(g.to_owned()),
            ..Subject::default()
        };
        assert_eq!(
            Agreement::Absent,
            compare(Field::Gender, &gender("male"), &Subject::default()).agreement
        );
        assert_eq!(
            Agreement::Disagree,
            compare(Field::Gender, &gender("male"), &gender("female")).agreement
        );
        let phones = |numbers: &[&str]| Subject {
            telephones: numbers.iter().map(|n| (*n).to_owned()).collect(),
            ..Subject::default()
        };
        assert_eq!(
            Agreement::Agree,
            compare(
                Field::Telephone,
                &phones(&["611111111", "622222222"]),
                &phones(&["622222222"])
            )
            .agreement
        );
    }
}
