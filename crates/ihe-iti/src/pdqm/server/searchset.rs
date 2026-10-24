// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-78 answer: a `searchset` Bundle of Patients with the `total` and
//! the paging links (§2:3.78.4.2.2, the Query Patient Resource Response
//! Message profile).

use std::fmt;

use fhir_types::r4::bundle::{Bundle, BundleEntry, BundleEntrySearch, BundleLink};
use fhir_types::r4::patient::Patient;
use fhir_types::r4::resource::Resource;
use http::StatusCode;
use url::Url;

use super::Score;
use super::search::PatientSearch;
use crate::server::answer::Answer;
use ihe_atna::redact::REDACTED;

/// How an entry came into the result set (FHIR R4 `search-entry-mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Match,
    Include,
}

/// One Patient of a result set.
///
/// `Debug` shows the mode and the score and redacts the rest; the type has no
/// `Display`.
#[derive(Clone)]
pub struct SearchEntry {
    full_url: Url,
    patient: Box<Patient>,
    mode: Mode,
    score: Option<Score>,
}

impl SearchEntry {
    /// Creates the entry of a Patient the search matched, at its absolute URL
    /// `full_url` on the Supplier (the profile's `Bundle.entry.fullUrl`).
    #[must_use]
    pub fn matched(full_url: Url, patient: Patient) -> Self {
        Self {
            full_url,
            patient: Box::new(patient),
            mode: Mode::Match,
            score: None,
        }
    }

    /// Creates the entry of a Patient the search did not match and the
    /// answer includes: the survivor a deprecated Patient's `replaced-by`
    /// link points at (§2:3.78.4.1.3, Case 6).
    #[must_use]
    pub fn included(full_url: Url, patient: Patient) -> Self {
        Self {
            mode: Mode::Include,
            ..Self::matched(full_url, patient)
        }
    }

    /// This entry, conveying the quality of the match as `score`
    /// (§2:3.78.4.2.2.5).
    #[must_use]
    pub fn scored(mut self, score: Score) -> Self {
        self.score = Some(score);
        self
    }
}

impl fmt::Debug for SearchEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchEntry")
            .field("full_url", &REDACTED)
            .field("patient", &REDACTED)
            .field("mode", &self.mode)
            .field("score", &self.score)
            .finish()
    }
}

/// The paging links of one page (FHIR R4 paging,
/// <http://hl7.org/fhir/R4/http.html#paging>; §2:3.78.4.2.2.4).
///
/// The links are the Supplier's own and may carry the search, so `Debug`
/// says which are present and never a URL.
#[derive(Clone, Default)]
pub struct Paging {
    first: Option<Url>,
    previous: Option<Url>,
    next: Option<Url>,
    last: Option<Url>,
}

impl Paging {
    /// Creates paging with no link: a result set on one page.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// This paging, with the `first` link.
    #[must_use]
    pub fn first(mut self, url: Url) -> Self {
        self.first = Some(url);
        self
    }

    /// This paging, with the `previous` link.
    #[must_use]
    pub fn previous(mut self, url: Url) -> Self {
        self.previous = Some(url);
        self
    }

    /// This paging, with the `next` link.
    #[must_use]
    pub fn next(mut self, url: Url) -> Self {
        self.next = Some(url);
        self
    }

    /// This paging, with the `last` link.
    #[must_use]
    pub fn last(mut self, url: Url) -> Self {
        self.last = Some(url);
        self
    }
}

impl fmt::Debug for Paging {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Paging")
            .field("first", &self.first.is_some())
            .field("previous", &self.previous.is_some())
            .field("next", &self.next.is_some())
            .field("last", &self.last.is_some())
            .finish()
    }
}

/// Returns the `200` answer to `search`: a `searchset` Bundle.
///
/// Its `total` is `total` (§2:3.78.4.1.3, Cases 1 to 3), with the `self`
/// link of what the search respected at `endpoint`, the Supplier's `[base]/Patient`
/// (§2:3.78.4.1.2.1.1), the `paging` links, and one entry per Patient of
/// this page in the order given, each with its `fullUrl` and search mode.
///
/// When the search names identifier domains, every Patient's identifiers
/// outside them are left out (§2:3.78.4.1.3, Case 2).
///
/// # Errors
/// [`SearchSetError::TotalBelowMatches`] when the page holds more matched
/// Patients than `total` counts.
pub fn searchset(
    search: &PatientSearch,
    endpoint: &Url,
    total: u32,
    entries: Vec<SearchEntry>,
    paging: &Paging,
) -> Result<Answer<Bundle>, SearchSetError> {
    let matched = entries
        .iter()
        .filter(|entry| entry.mode == Mode::Match)
        .count();
    if u32::try_from(matched).map_or(true, |matched| matched > total) {
        return Err(SearchSetError::TotalBelowMatches);
    }
    let link = |relation: &str, url: &Url| BundleLink {
        relation: relation.into(),
        url: url.as_str().into(),
        ..BundleLink::default()
    };
    let mut links = vec![link("self", search.self_link(endpoint).url())];
    for (relation, url) in [
        ("first", &paging.first),
        ("previous", &paging.previous),
        ("next", &paging.next),
        ("last", &paging.last),
    ] {
        if let Some(url) = url {
            links.push(link(relation, url));
        }
    }
    let entry = entries
        .into_iter()
        .map(|entry| {
            let mut patient = entry.patient;
            prune(&mut patient, search.domains());
            BundleEntry {
                full_url: Some(entry.full_url.as_str().into()),
                resource: Some(Resource::Patient(patient)),
                search: Some(BundleEntrySearch {
                    mode: Some(
                        match entry.mode {
                            Mode::Match => "match",
                            Mode::Include => "include",
                        }
                        .into(),
                    ),
                    score: entry.score.map(|score| score.decimal().into()),
                    ..BundleEntrySearch::default()
                }),
                ..BundleEntry::default()
            }
        })
        .collect();
    Ok(Answer::new(
        StatusCode::OK,
        Bundle {
            r#type: "searchset".into(),
            total: Some(total.into()),
            link: links,
            entry,
            ..Bundle::default()
        },
    ))
}

/// Leaves out the identifiers of `patient` outside `domains`, when there are
/// any (§2:3.78.4.1.2.3).
fn prune(patient: &mut Patient, domains: &[String]) {
    if domains.is_empty() {
        return;
    }
    patient.identifier.retain(|identifier| {
        identifier
            .system
            .as_ref()
            .and_then(|system| system.value.as_deref())
            .is_some_and(|system| domains.iter().any(|domain| domain == system))
    });
}

/// Why a result set cannot be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SearchSetError {
    /// The page holds more matched Patients than the `total` counts.
    #[error("the page holds more matches than the total")]
    TotalBelowMatches,
}

#[cfg(test)]
mod tests {
    use fhir_types::r4::identifier::Identifier;
    use fhir_types::r4::patient::Patient;
    use url::Url;

    use super::{Paging, SearchEntry, SearchSetError, searchset};
    use crate::pdqm::server::search::PatientSearch;
    use crate::server::format::Format;

    fn patient() -> Patient {
        let identifier = |system: &str, value: &str| Identifier {
            system: Some(system.into()),
            value: Some(value.into()),
            ..Identifier::default()
        };
        Patient {
            id: Some("p1".into()),
            identifier: vec![
                identifier("urn:oid:2.999.1", "1"),
                identifier("urn:oid:2.999.2", "2"),
            ],
            ..Patient::default()
        }
    }

    fn url(text: &str) -> Url {
        Url::parse(text).expect("a URL")
    }

    #[test]
    fn the_domain_filter_prunes_the_identifiers_of_every_patient() {
        let search = PatientSearch::from_query("identifier=urn:oid:2.999.2%7C").expect("a search");
        let answer = searchset(
            &search,
            &url("https://pdq.example.org/fhir/Patient"),
            1,
            vec![SearchEntry::matched(
                url("https://pdq.example.org/fhir/Patient/p1"),
                patient(),
            )],
            &Paging::new().next(url("https://pdq.example.org/fhir/Patient?page=2")),
        )
        .expect("a searchset");
        let json = String::from_utf8(answer.encode(Format::Json).expect("JSON")).expect("UTF-8");
        assert!(json.contains(r#""total":1"#), "{json}");
        assert!(json.contains(r#""relation":"next""#), "{json}");
        assert!(
            !json.contains("urn:oid:2.999.1\""),
            "the other domain is pruned: {json}"
        );
        assert!(json.contains(r#""mode":"match""#), "{json}");
    }

    #[test]
    fn a_total_below_the_page_is_refused() {
        let search = PatientSearch::from_query("family=MOHR").expect("a search");
        let refused = searchset(
            &search,
            &url("https://pdq.example.org/fhir/Patient"),
            0,
            vec![SearchEntry::matched(
                url("https://pdq.example.org/fhir/Patient/p1"),
                patient(),
            )],
            &Paging::new(),
        );
        assert_eq!(refused.err(), Some(SearchSetError::TotalBelowMatches));
    }
}
