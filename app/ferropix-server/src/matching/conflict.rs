// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The hard conflicts that keep a shared authoritative identifier from
//! linking two records alone.
//!
//! A birth year that differs, or an administrative gender that differs
//! where both records state one, turns the deterministic link into a
//! candidate for review, with the conflict as the first line of its
//! evidence (`docs/architecture.md` §5.1). No specification governs the
//! conflicts: our own design.

use serde::Serialize;

use crate::matching::subject::Subject;

/// A demographic difference no shared authoritative identifier outweighs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    /// The birth years differ.
    BirthYear,
    /// The administrative genders differ.
    Gender,
}

/// Returns the hard conflicts between `a` and `b`, in order.
#[must_use]
pub fn conflicts(a: &Subject, b: &Subject) -> Vec<Conflict> {
    let mut found = Vec::new();
    if let (Some(x), Some(y)) = (a.birth, b.birth)
        && x.year != y.year
    {
        found.push(Conflict::BirthYear);
    }
    if let (Some(x), Some(y)) = (&a.gender, &b.gender)
        && x != y
    {
        found.push(Conflict::Gender);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{Conflict, conflicts};
    use crate::matching::subject::{Birth, Subject};

    fn subject(year: Option<u16>, gender: Option<&str>) -> Subject {
        Subject {
            birth: year.map(|year| Birth { year, day: None }),
            gender: gender.map(str::to_owned),
            ..Subject::default()
        }
    }

    #[test]
    fn a_differing_year_or_gender_is_a_conflict_and_an_absent_one_is_not() {
        let a = subject(Some(1958), Some("female"));
        assert_eq!(
            Vec::<Conflict>::new(),
            conflicts(&a, &subject(Some(1958), Some("female")))
        );
        assert_eq!(Vec::<Conflict>::new(), conflicts(&a, &subject(None, None)));
        assert_eq!(
            vec![Conflict::BirthYear, Conflict::Gender],
            conflicts(&a, &subject(Some(1959), Some("male")))
        );
    }
}
