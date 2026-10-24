// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The scorer over a corpus of synthetic pairs, and its properties.
//!
//! Every name is one of the testkit's invented names, or one of them with a
//! letter changed, and every date and number is made up for the case. The
//! expected totals are recomputed here from the weights the book documents
//! (`website/book/src/operate/matching.md`), so a changed default fails the
//! corpus until the documentation follows it.

use std::collections::BTreeSet;

use ferropix_server::config::section::matching::{Field, Matching};
use ferropix_server::matching::compare::Agreement;
use ferropix_server::matching::scorer::{Grade, Scorer};
use ferropix_server::matching::subject::{IdentifierKey, Subject};
use ferropix_testkit::synthetic::{FAMILY_NAMES, GIVEN_NAMES};
use fhir_types::codec::{Json as _, Path, Value};
use fhir_types::r4::patient::Patient;
use proptest::prelude::*;

/// The default `m` and `u` of each field, read from the book's table.
fn documented_weights() -> Vec<(String, f64, f64)> {
    let page = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../website/book/src/operate/matching.md");
    let text = std::fs::read_to_string(page).unwrap();
    let rows: Vec<(String, f64, f64)> = text
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let field = cells.get(1)?.strip_prefix('`')?.strip_suffix('`')?;
            let m = cells.get(2)?.parse().ok()?;
            let u = cells.get(3)?.parse().ok()?;
            Some((field.to_owned(), m, u))
        })
        .collect();
    assert_eq!(10, rows.len(), "the book's table names every field");
    rows
}

/// The synthetic patient a pair is built from.
#[derive(Clone, Default)]
struct Person {
    family: Option<String>,
    given: Vec<String>,
    birth: Option<&'static str>,
    gender: Option<&'static str>,
    postal: Option<&'static str>,
    line: Option<&'static str>,
    phone: Option<&'static str>,
}

impl Person {
    fn patient(&self) -> Patient {
        let mut object = serde_json::Map::new();
        object.insert("resourceType".into(), "Patient".into());
        if self.family.is_some() || !self.given.is_empty() {
            let mut name = serde_json::Map::new();
            if let Some(family) = &self.family {
                name.insert("family".into(), family.clone().into());
            }
            if !self.given.is_empty() {
                name.insert("given".into(), self.given.clone().into());
            }
            object.insert("name".into(), vec![serde_json::Value::Object(name)].into());
        }
        if let Some(birth) = self.birth {
            object.insert("birthDate".into(), birth.into());
        }
        if let Some(gender) = self.gender {
            object.insert("gender".into(), gender.into());
        }
        if self.postal.is_some() || self.line.is_some() {
            let mut address = serde_json::Map::new();
            if let Some(postal) = self.postal {
                address.insert("postalCode".into(), postal.into());
            }
            if let Some(line) = self.line {
                address.insert("line".into(), vec![line].into());
            }
            object.insert(
                "address".into(),
                vec![serde_json::Value::Object(address)].into(),
            );
        }
        if let Some(phone) = self.phone {
            let point = serde_json::json!({"system": "phone", "value": phone});
            object.insert("telecom".into(), vec![point].into());
        }
        let text = serde_json::Value::Object(object).to_string();
        let value: Value = serde_json::from_str(&text).unwrap();
        Patient::from_json(value.as_object().unwrap(), &mut Path::root("Patient")).unwrap()
    }

    fn subject(&self) -> Subject {
        Subject::from_patient(&self.patient(), BTreeSet::new())
    }
}

/// The base of every pair: a synthetic patient with every field the
/// corpus varies.
fn base(index: usize) -> Person {
    Person {
        family: Some(FAMILY_NAMES[index % FAMILY_NAMES.len()].to_owned()),
        given: vec![GIVEN_NAMES[index % GIVEN_NAMES.len()].to_owned()],
        birth: Some("1958-03-14"),
        gender: Some("female"),
        postal: Some("1234 AB"),
        line: Some("Proefkade 12"),
        phone: Some("06-12345678"),
    }
}

/// Returns `name` with its last letter doubled, a typing slip.
fn slip(name: &str) -> String {
    let last = name.chars().last().unwrap();
    format!("{name}{last}")
}

/// The documented weight of `field` at `agreement`.
fn documented(field: Field, agreement: Agreement) -> f64 {
    let weights = documented_weights();
    let (_, m, u) = weights.iter().find(|(f, _, _)| f == field.key()).unwrap();
    match agreement {
        Agreement::Agree => (m / u).log2(),
        Agreement::Partial => (m / u).log2() / 2.0,
        Agreement::Disagree => ((1.0 - m) / (1.0 - u)).log2(),
        Agreement::Absent => 0.0,
    }
}

/// One pair of the corpus with the grade the documented weights give it.
struct Pair {
    name: &'static str,
    a: Person,
    b: Person,
    grade: Grade,
}

fn corpus() -> Vec<Pair> {
    let a = base(0);
    vec![
        Pair {
            name: "the same patient twice",
            a: a.clone(),
            b: a.clone(),
            grade: Grade::Certain,
        },
        Pair {
            name: "a slip in the family name, the rest equal",
            a: a.clone(),
            b: Person {
                family: a.family.as_deref().map(slip),
                ..a.clone()
            },
            grade: Grade::Certain,
        },
        Pair {
            name: "names and the birth date alone",
            a: Person {
                postal: None,
                line: None,
                phone: None,
                gender: None,
                ..a.clone()
            },
            b: Person {
                postal: None,
                line: None,
                phone: None,
                gender: None,
                ..a.clone()
            },
            grade: Grade::Certain,
        },
        Pair {
            name: "the family and given names alone",
            a: Person {
                family: a.family.clone(),
                given: a.given.clone(),
                ..Person::default()
            },
            b: Person {
                family: a.family.clone(),
                given: a.given.clone(),
                ..Person::default()
            },
            grade: Grade::Probable,
        },
        Pair {
            name: "the family name and the birth date, another given name",
            a: Person {
                family: a.family.clone(),
                given: a.given.clone(),
                birth: a.birth,
                ..Person::default()
            },
            b: Person {
                family: a.family.clone(),
                given: vec![GIVEN_NAMES[6].to_owned()],
                birth: a.birth,
                ..Person::default()
            },
            grade: Grade::Possible,
        },
        Pair {
            name: "the same family name, everything else different",
            a: a.clone(),
            b: Person {
                given: vec![GIVEN_NAMES[5].to_owned()],
                birth: Some("1971-09-02"),
                gender: Some("male"),
                postal: Some("9876 ZX"),
                line: Some("Steekproefweg 3"),
                phone: Some("06-99887766"),
                ..a.clone()
            },
            grade: Grade::CertainlyNot,
        },
        Pair {
            name: "two different patients",
            a: a.clone(),
            b: Person {
                family: Some(FAMILY_NAMES[4].to_owned()),
                given: vec![GIVEN_NAMES[3].to_owned()],
                birth: Some("1990-12-01"),
                gender: Some("male"),
                postal: Some("5555 QQ"),
                line: Some("Monsterlaan 8"),
                phone: Some("06-55554444"),
            },
            grade: Grade::CertainlyNot,
        },
    ]
}

#[test]
fn the_corpus_scores_with_the_documented_weights_and_grades() {
    let scorer = Scorer::new(&Matching::default());
    for pair in corpus() {
        let found = scorer.score(&pair.a.subject(), &pair.b.subject(), None);
        let total: f64 = found
            .evidence
            .fields
            .iter()
            .map(|field| documented(field.field, field.agreement))
            .sum();
        for field in &found.evidence.fields {
            let expected = documented(field.field, field.agreement);
            assert!(
                (field.weight - expected).abs() < 1e-9,
                "{}: {:?} weighs {} and the book says {expected}",
                pair.name,
                field.field,
                field.weight
            );
        }
        let score = 1.0 / (1.0 + (10.0_f64 - total).exp2());
        assert!((found.score - score).abs() < 1e-12, "{}", pair.name);
        assert_eq!(
            pair.grade, found.grade,
            "{}: score {}",
            pair.name, found.score
        );
    }
}

#[test]
fn the_corpus_scores_bit_for_bit_the_same_on_every_run() {
    let scorer = Scorer::new(&Matching::default());
    for pair in corpus() {
        let first = scorer.score(&pair.a.subject(), &pair.b.subject(), None);
        let again =
            Scorer::new(&Matching::default()).score(&pair.a.subject(), &pair.b.subject(), None);
        assert_eq!(
            first.score.to_bits(),
            again.score.to_bits(),
            "{}",
            pair.name
        );
        assert_eq!(
            serde_json::to_string(&first.evidence).unwrap(),
            serde_json::to_string(&again.evidence).unwrap(),
            "{}",
            pair.name
        );
    }
}

#[test]
fn a_changed_threshold_moves_the_grade_without_a_code_change() {
    let pair = corpus()
        .into_iter()
        .find(|p| p.grade == Grade::Probable)
        .unwrap();
    let (a, b) = (pair.a.subject(), pair.b.subject());
    let default = Scorer::new(&Matching::default()).score(&a, &b, None);
    assert_eq!(Grade::Probable, default.grade);
    let lower: Matching =
        toml::from_str("link_at = 0.8\nreview_at = 0.7\npossible_at = 0.5\n").unwrap();
    let lower = lower.resolve().unwrap();
    assert_eq!(
        Grade::Certain,
        Scorer::new(&lower).score(&a, &b, None).grade
    );
    let higher: Matching =
        toml::from_str("link_at = 0.999\nreview_at = 0.99\npossible_at = 0.6\n").unwrap();
    let higher = higher.resolve().unwrap();
    assert_eq!(
        Grade::Possible,
        Scorer::new(&higher).score(&a, &b, None).grade
    );
}

#[test]
#[expect(
    clippy::disallowed_types,
    reason = "the evidence is read back as the JSON the candidate column stores"
)]
fn every_candidate_carries_one_entry_per_field_with_masked_values_and_its_blocking_key() {
    let scorer = Scorer::new(&Matching::default());
    let a = base(1);
    let key = Some(ferropix_server::config::section::matching::BlockingKey::PostalCodeBirthDate);
    let found = scorer.score(&a.subject(), &a.subject(), key);
    let json: serde_json::Value = serde_json::to_value(&found.evidence).unwrap();
    assert_eq!("postal_code_birth_date", json["blocking_key"], "{json}");
    let fields = json["fields"].as_array().unwrap();
    assert_eq!(10, fields.len());
    let family = a.family.unwrap().to_lowercase();
    for field in fields {
        for member in ["field", "comparator", "agreement", "weight"] {
            assert!(field.get(member).is_some(), "{member} in {field}");
        }
        for side in ["a", "b"] {
            if let Some(value) = field.get(side).and_then(serde_json::Value::as_str) {
                assert!(value.starts_with('•'), "{value} is masked");
            }
        }
    }
    assert!(!json.to_string().contains(&family), "no family name whole");
    let family_entry = &fields[0];
    assert_eq!("family_name", family_entry["field"]);
    assert_eq!("jaro-winkler", family_entry["comparator"]);
    assert_eq!("agree", family_entry["agreement"]);
}

/// A subject drawn from the synthetic set, any field present or absent.
fn subjects() -> impl Strategy<Value = Subject> {
    (
        proptest::option::of(0..FAMILY_NAMES.len()),
        proptest::option::of(0..GIVEN_NAMES.len()),
        proptest::option::of((1950_u16..1960, 1_u8..13, 1_u8..29)),
        proptest::option::of(prop_oneof![Just("male"), Just("female")]),
        proptest::option::of(prop_oneof![Just("1234AB"), Just("1234ZZ"), Just("9876XY")]),
        proptest::option::of(0_u8..3),
    )
        .prop_map(|(family, given, birth, gender, postal, local)| {
            let person = Person {
                family: family.map(|i| FAMILY_NAMES[i].to_owned()),
                given: given
                    .map(|i| GIVEN_NAMES[i].to_owned())
                    .into_iter()
                    .collect(),
                birth: None,
                gender,
                postal,
                ..Person::default()
            };
            let mut subject = person.subject();
            subject.birth =
                birth.map(
                    |(year, month, day)| ferropix_server::matching::subject::Birth {
                        year,
                        day: Some((month, day)),
                    },
                );
            if let Some(value) = local {
                subject
                    .local_identifiers
                    .insert(IdentifierKey::new(uuid::Uuid::from_u128(7), [value; 32]));
            }
            subject
        })
}

proptest! {
    #[test]
    fn a_score_is_symmetric_in_the_unit_interval_and_graded_by_the_thresholds(
        a in subjects(),
        b in subjects(),
    ) {
        let scorer = Scorer::new(&Matching::default());
        let ab = scorer.score(&a, &b, None);
        let ba = scorer.score(&b, &a, None);
        prop_assert!((0.0..=1.0).contains(&ab.score));
        prop_assert_eq!(ab.score.to_bits(), ba.score.to_bits());
        prop_assert_eq!(ab.grade, ba.grade);
        prop_assert_eq!(ab.grade, scorer.grade(ab.score));
        let total: f64 = ab.evidence.fields.iter().map(|f| f.weight).sum();
        prop_assert_eq!(total.to_bits(), ab.evidence.total.to_bits());
    }

    #[test]
    fn a_subject_scores_itself_at_least_as_high_as_any_other(a in subjects(), b in subjects()) {
        let scorer = Scorer::new(&Matching::default());
        prop_assert!(scorer.score(&a, &a, None).score >= scorer.score(&a, &b, None).score);
    }

    #[test]
    fn a_field_that_starts_to_agree_never_lowers_the_score(a in subjects(), b in subjects()) {
        let scorer = Scorer::new(&Matching::default());
        let before = scorer.score(&a, &b, None).score;
        let mut agreeing = b.clone();
        agreeing.gender.clone_from(&a.gender);
        if a.gender.is_some() {
            prop_assert!(scorer.score(&a, &agreeing, None).score >= before);
        }
    }
}
