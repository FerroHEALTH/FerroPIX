// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The PDQm Patient Demographics Supplier with the Match Operation Option,
//! over the store (PDQm 3.2.0 §1:38.1.1.2; `docs/architecture.md` §6.2).
//!
//! - ITI-78, the search ([`Supplier::search`]): the persons matching every
//!   criterion, a page of them in a `searchset` Bundle with its `total`,
//!   its `self` link and its paging links (§2:3.78.4.1.3,
//!   §2:3.78.4.2.2.4), the identifiers pruned to the domain filter
//!   (§2:3.78.4.1.2.3), and a merged person answered by the
//!   `[pdqm] deprecated` policy (§2:3.78.4.1.3, Case 6).
//! - ITI-78, the retrieve ([`Supplier::retrieve`]): one person by its id
//!   (§2:3.78.4.3.3).
//! - ITI-119, `$match` ([`Supplier::matches`]): the input scored by the
//!   scorer, one entry per person with its `search.score` and `match-grade`
//!   (§2:3.119.4.1.3, §2:3.119.4.2.2.4).
//!
//! [`patient`] builds the `Patient` each answer serves, [`search`] runs the
//! search over the store and [`matching`] the match. Each answer is read in
//! one read-only snapshot, so a page, its total and its Patients agree.
//! Nothing here writes to the store.

pub mod matching;
pub mod patient;
pub mod search;

use std::collections::BTreeSet;

use fhir_types::r4::bundle::Bundle;
use fhir_types::r4::operation_outcome::{OperationOutcome, OperationOutcomeIssue};
use fhir_types::r4::patient::Patient;
use fhir_types::r4::resource::Resource;
use ihe_iti::pdqm::matches::MatchGrade;
use ihe_iti::pdqm::server::matching::{MatchEntry, MatchRequest, MatchSetError, match_set};
use ihe_iti::pdqm::server::search::PatientSearch;
use ihe_iti::pdqm::server::searchset::{Paging, SearchEntry, SearchSetError, searchset};
use ihe_iti::pdqm::server::{Refusal, Score, ScoreError, retrieved};
use ihe_iti::server::answer::Answer;
use sqlx::{PgConnection, Postgres, Transaction};
use url::Url;
use uuid::Uuid;

use crate::config::section::matching::{BlockingKey, Matching};
use crate::config::section::profiles::Deprecated;
use crate::feed::demographics::DemographicsError;
use crate::matching::scorer::{Grade, Scorer};
use crate::pdqm::patient::Person;
use crate::store::record;
use crate::store::{Store, StoreError};

/// The persons a page holds when `_count` names none (FHIR R4 search,
/// paging; the number is our own design).
pub const DEFAULT_PAGE: u32 = 20;

/// The most persons a page holds, whatever `_count` asks: FHIR R4 lets a
/// server return fewer (the number is our own design).
pub const MAX_PAGE: u32 = 100;

/// The parameter a paging link names the last person of the page before
/// with (FHIR R4 http.html, paging: the links are the server's own).
pub const PAGE_PARAMETER: &str = "_page";

/// The diagnostics of the warning an input with too few fields carries
/// (PDQm 3.2.0 §2:3.119.4.1.3, Case 10).
pub const FEW_FIELDS_WARNING: &str =
    "the input Patient states too few demographics for a reliable match";

/// The Supplier over one store.
#[derive(Debug, Clone)]
pub struct Supplier {
    /// The store.
    store: Store,
    /// The scorer of the `[matching]` section.
    scorer: Scorer,
    /// The blocking keys, in the configured order.
    blocking: Vec<BlockingKey>,
    /// How a merged person is answered.
    deprecated: Deprecated,
    /// The system of the Master Patient Identification Domain: the public
    /// base URL.
    master: String,
    /// The FHIR base, `{public_base_url}/fhir`.
    fhir_base: Url,
}

/// What a search answered.
#[derive(Debug, Clone)]
pub enum Searched {
    /// The search was run; the persons are those on the page, in order.
    Found {
        /// The `searchset` Bundle.
        answer: Box<Answer<Bundle>>,
        /// The persons matched on the page.
        persons: Vec<Uuid>,
    },
    /// The search was refused as the transaction prescribes.
    Refused(Refusal),
}

/// What a retrieve answered.
#[derive(Debug, Clone)]
pub enum Retrieved {
    /// The person, found.
    Found {
        /// The `Patient`.
        answer: Box<Answer<Patient>>,
        /// The person.
        person: Uuid,
    },
    /// No person has the id, or it is merged and the policy omits it.
    NotFound,
}

/// What a match answered.
#[derive(Debug, Clone)]
pub struct Matches {
    /// The Match Output Bundle.
    pub answer: Answer<Bundle>,
    /// The persons the Bundle names, in its order.
    pub persons: Vec<Uuid>,
}

impl Supplier {
    /// Returns the Supplier over `store`, scoring with `matching`, answering
    /// a merged person by `deprecated`, at `public_base_url`.
    #[must_use]
    pub fn new(
        store: Store,
        matching: &Matching,
        deprecated: Deprecated,
        public_base_url: &Url,
    ) -> Self {
        let master = public_base_url.as_str().trim_end_matches('/').to_owned();
        let fhir_base =
            Url::parse(&format!("{master}/fhir")).unwrap_or_else(|_| public_base_url.clone());
        Self {
            store,
            scorer: Scorer::new(matching),
            blocking: matching.blocking_keys.clone(),
            deprecated,
            master,
            fhir_base,
        }
    }

    /// Returns the FHIR base the answers' URLs are written under.
    #[must_use]
    pub fn fhir_base(&self) -> &Url {
        &self.fhir_base
    }

    /// Returns the absolute URL of `rest` under the FHIR base.
    #[must_use]
    pub fn url(&self, rest: &str) -> Url {
        let base = self.fhir_base.as_str().trim_end_matches('/');
        Url::parse(&format!("{base}/{rest}")).unwrap_or_else(|_| self.fhir_base.clone())
    }

    /// Runs the ITI-78 search `search` and answers the page after `after`.
    ///
    /// # Errors
    /// Returns [`PdqError`] when the store cannot be read or the answer
    /// cannot be written.
    pub async fn search(
        &self,
        search: &PatientSearch,
        after: Option<Uuid>,
    ) -> Result<Searched, PdqError> {
        let mut snapshot = self.snapshot().await?;
        if !self.recognises(&mut snapshot, search.domains()).await? {
            return Ok(Searched::Refused(Refusal::DomainNotRecognized));
        }
        let size = search
            .count()
            .map_or(DEFAULT_PAGE, |count| count.min(MAX_PAGE));
        let page = search::run(
            &mut snapshot,
            self.store.sealer(),
            &self.master,
            search,
            search::Window {
                size,
                after,
                omit_merged: self.deprecated == Deprecated::Omit,
            },
        )
        .await?;
        let found = patient::persons(&mut snapshot, &page.persons).await?;
        let mut wanted: Vec<Person> = page
            .persons
            .iter()
            .filter_map(|id| found.get(id).copied())
            .collect();
        let on_page: BTreeSet<Uuid> = page.persons.iter().copied().collect();
        let survivors: BTreeSet<Uuid> = wanted
            .iter()
            .filter_map(|person| person.survivor)
            .filter(|survivor| !on_page.contains(survivor))
            .collect();
        wanted.extend(survivors.iter().map(|id| Person {
            id: *id,
            survivor: None,
        }));
        let mut patients =
            patient::patients(&mut snapshot, self.store.sealer(), &self.master, &wanted).await?;
        snapshot.commit().await.map_err(store)?;
        let mut entries = Vec::with_capacity(wanted.len());
        let mut included = BTreeSet::new();
        for person in wanted.iter().filter(|person| on_page.contains(&person.id)) {
            if let Some(patient) = patients.remove(&person.id) {
                entries.push(SearchEntry::matched(self.patient_url(person.id), patient));
            }
            if let Some(survivor) = person.survivor.filter(|s| included.insert(*s))
                && let Some(patient) = patients.remove(&survivor)
            {
                entries.push(SearchEntry::included(self.patient_url(survivor), patient));
            }
        }
        let endpoint = self.url("Patient");
        let first = search.self_link(&endpoint).url().clone();
        let mut paging = Paging::new().first(first.clone());
        if page.more
            && size > 0
            && let Some(last) = page.persons.last()
        {
            let mut next = first;
            next.query_pairs_mut()
                .append_pair(PAGE_PARAMETER, &last.to_string());
            paging = paging.next(next);
        }
        let answer = searchset(search, &endpoint, page.total, entries, &paging)?;
        Ok(Searched::Found {
            answer: Box::new(answer),
            persons: page.persons,
        })
    }

    /// Retrieves the person `id` names (PDQm 3.2.0 §2:3.78.4.3.3).
    ///
    /// A merged person is answered with `active = false` under the
    /// `inactive` policy and as not found under `omit` (Case 3).
    ///
    /// # Errors
    /// Returns [`PdqError`] when the store cannot be read.
    pub async fn retrieve(&self, id: &str) -> Result<Retrieved, PdqError> {
        let Ok(id) = Uuid::try_parse(id) else {
            return Ok(Retrieved::NotFound);
        };
        let mut snapshot = self.snapshot().await?;
        let Some(person) = patient::persons(&mut snapshot, &[id]).await?.remove(&id) else {
            return Ok(Retrieved::NotFound);
        };
        if person.merged() && self.deprecated == Deprecated::Omit {
            return Ok(Retrieved::NotFound);
        }
        let patients =
            patient::patients(&mut snapshot, self.store.sealer(), &self.master, &[person]).await?;
        snapshot.commit().await.map_err(store)?;
        Ok(match patients.into_values().next() {
            Some(patient) => Retrieved::Found {
                answer: Box::new(retrieved(patient)),
                person: id,
            },
            None => Retrieved::NotFound,
        })
    }

    /// Runs the ITI-119 match `request` (PDQm 3.2.0 §2:3.119.4.1.3).
    ///
    /// # Errors
    /// Returns [`PdqError`] when the store cannot be read or the answer
    /// cannot be written.
    pub async fn matches(&self, request: &MatchRequest) -> Result<Matches, PdqError> {
        let mut snapshot = self.snapshot().await?;
        let matched = matching::run(
            &mut snapshot,
            self.store.sealer(),
            &self.scorer,
            &self.blocking,
            request.patient(),
        )
        .await?;
        let ids: Vec<Uuid> = matched.found.iter().map(|found| found.person).collect();
        let persons = patient::persons(&mut snapshot, &ids).await?;
        let wanted: Vec<Person> = ids
            .iter()
            .filter_map(|id| persons.get(id).copied())
            .collect();
        let mut patients =
            patient::patients(&mut snapshot, self.store.sealer(), &self.master, &wanted).await?;
        snapshot.commit().await.map_err(store)?;
        let mut entries = Vec::with_capacity(matched.found.len());
        for found in &matched.found {
            if let Some(patient) = patients.remove(&found.person) {
                entries.push(MatchEntry::new(
                    self.patient_url(found.person),
                    patient,
                    Score::new(found.score)?,
                    grade(found.grade),
                ));
            }
        }
        let warnings = if matched.few_fields {
            vec![few_fields()]
        } else {
            Vec::new()
        };
        let answer = match_set(
            request,
            entries,
            warnings,
            Some(&self.url("Patient/$match")),
        )?;
        let persons = answer
            .resource()
            .entry
            .iter()
            .filter_map(|entry| match &entry.resource {
                Some(Resource::Patient(patient)) => patient.id.as_deref(),
                _ => None,
            })
            .filter_map(|id| Uuid::try_parse(id).ok())
            .collect();
        Ok(Matches { answer, persons })
    }

    /// Returns the absolute URL of the person `id`'s `Patient`.
    fn patient_url(&self, id: Uuid) -> Url {
        self.url(&format!("Patient/{id}"))
    }

    /// Returns whether every domain of a domain filter is one the Supplier
    /// is an authority for: a domain the store holds and has not retired,
    /// or the Master Patient Identification Domain (PDQm 3.2.0
    /// §2:3.78.4.1.3, Case 4).
    async fn recognises(
        &self,
        connection: &mut PgConnection,
        domains: &[String],
    ) -> Result<bool, PdqError> {
        if domains.is_empty() {
            return Ok(true);
        }
        let known: BTreeSet<String> = record::domains(&mut *connection)
            .await?
            .into_iter()
            .filter(|domain| !domain.retired)
            .map(|domain| domain.system)
            .collect();
        Ok(domains
            .iter()
            .all(|system| *system == self.master || known.contains(system)))
    }

    /// Opens a read-only transaction that sees one snapshot of the store.
    async fn snapshot(&self) -> Result<Transaction<'static, Postgres>, PdqError> {
        let mut transaction = self.store.pool().begin().await.map_err(store)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(store)?;
        Ok(transaction)
    }
}

/// Returns the `match-grade` of a scorer's `grade` (PDQm 3.2.0
/// §2:3.119.4.2.2.4): the scorer grades with the value set's own codes.
#[must_use]
pub const fn grade(grade: Grade) -> MatchGrade {
    match grade {
        Grade::Certain => MatchGrade::Certain,
        Grade::Probable => MatchGrade::Probable,
        Grade::Possible => MatchGrade::Possible,
        Grade::CertainlyNot => MatchGrade::CertainlyNot,
    }
}

/// Returns the warning of an input with too few fields (PDQm 3.2.0
/// §2:3.119.4.1.3, Case 10).
fn few_fields() -> OperationOutcome {
    OperationOutcome {
        issue: vec![OperationOutcomeIssue {
            severity: "warning".into(),
            code: "incomplete".into(),
            diagnostics: Some(FEW_FIELDS_WARNING.into()),
            ..OperationOutcomeIssue::default()
        }],
        ..OperationOutcome::default()
    }
}

/// Why a PDQm request could not be answered.
///
/// No variant carries a value from the request or the store.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PdqError {
    /// A statement on the store failed, or a sealed identifier did not open.
    #[error("the store could not be read")]
    Store(#[from] StoreError),
    /// Stored demographics are no `Patient`.
    #[error("the stored demographics could not be read")]
    Demographics(#[from] DemographicsError),
    /// A criterion, modifier, prefix or code the search does not read.
    #[error("the search names a criterion this server does not read")]
    Unsupported,
    /// A score fell outside `[0, 1]`.
    #[error("a score fell outside the unit interval")]
    Score(#[from] ScoreError),
    /// The `searchset` could not be written.
    #[error("the searchset could not be written")]
    Searchset(#[from] SearchSetError),
    /// The Match Output Bundle could not be written.
    #[error("the match output could not be written")]
    MatchSet(#[from] MatchSetError),
}

/// Wraps a driver error.
fn store(source: sqlx::Error) -> PdqError {
    PdqError::Store(source.into())
}
