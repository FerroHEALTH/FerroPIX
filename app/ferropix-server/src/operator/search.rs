// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The two searches, both posted so no value sits in a URL.
//!
//! `POST {base}/operator/search/identifier` finds the persons one identifier
//! is cross-referenced to, as a PIX query does: by the HMAC of the value its
//! domain's rule normalises, so no sealed value is opened to search, an
//! authoritative identifier with no record of its own found through the
//! versions that carry it, and every person it names listed where a PIX query
//! answers the ambiguity alone. `POST {base}/operator/search/demographics`
//! runs the scorer over the blocking keys, as ITI-119 does, and lists the
//! persons the most likely first with their score and grade.
//!
//! Each person is shown with the demographics the PDQm Supplier serves and
//! its identifiers masked. A search that runs writes one BALP Patient Query
//! record per person found, or one Query record when none was, with its raw
//! request (BALP 1.1.4 §1:52.4.1.4, §1:52.4.1.6), before it answers; a
//! request refused before it runs writes none. No search writes a steward
//! action: the BALP trail holds it. These rules are our own design.

use std::collections::BTreeMap;

use axum::extract::Request;
use axum::response::Response;
use ferropix_operator_api::patient::{DomainRef, Masked, PersonStatus};
use ferropix_operator_api::search::{
    DEFAULT_LIMIT, DemographicsSearch, Hit, IdentifierSearch, MAX_LIMIT, MaskedIdentifier, Results,
};
use fhir_types::r4::address::Address;
use fhir_types::r4::human_name::HumanName;
use fhir_types::r4::patient::Patient;
use http::StatusCode;
use http::request::Parts;
use ihe_atna::balp::Outcome;
use secrecy::SecretString;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::audit::records;
use crate::fhir::outcome::IssueType;
use crate::matching::scorer::{Grade, mask};
use crate::operator::person::leading;
use crate::operator::{Operator, OperatorError, domain_kind, snapshot, wire};
use crate::pdqm::matching;
use crate::store::identifier::Rule;
use crate::store::{record, xref};

/// The diagnostics of an identifier search in a domain FerroPIX does not
/// know.
pub const UNKNOWN_DOMAIN: &str = "the identifier domain is unknown or retired";

/// The diagnostics of a demographics search that states no field.
pub const NO_FIELD: &str = "a demographics search states at least one field";

/// A body of either search, with the page it asks for.
#[derive(Debug, Clone, Copy)]
struct Window {
    /// The persons to skip.
    offset: u32,
    /// The persons a page holds.
    limit: u32,
}

impl Window {
    /// Returns the window of `offset` and `limit`, the limit bounded.
    fn of(offset: Option<u32>, limit: Option<u32>) -> Self {
        Self {
            offset: offset.unwrap_or(0),
            limit: limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
        }
    }

    /// Returns this window's page of `items` with the total and the next
    /// offset.
    fn page<T>(self, items: Vec<T>) -> (u32, Vec<T>, Option<u32>) {
        let total = u32::try_from(items.len()).unwrap_or(u32::MAX);
        let skip = usize::try_from(self.offset).unwrap_or(usize::MAX);
        let take = usize::try_from(self.limit).unwrap_or(usize::MAX);
        let end = self.offset.saturating_add(self.limit);
        let next = (end < total).then_some(end);
        (
            total,
            items.into_iter().skip(skip).take(take).collect(),
            next,
        )
    }
}

/// Answers `POST {base}/operator/search/identifier`.
pub async fn by_identifier(operator: &Operator, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let (search, bytes) = match wire::posted::<IdentifierSearch>(&parts, body).await {
        Ok(read) => read,
        Err(refused) => return *refused,
    };
    let window = Window::of(search.offset, search.limit);
    let found = match identifier_persons(operator, &search).await {
        Ok(Some(found)) => found,
        Ok(None) => {
            return wire::refuse(
                StatusCode::UNPROCESSABLE_ENTITY,
                IssueType::NotFound,
                UNKNOWN_DOMAIN,
            );
        }
        Err(error) => return wire::failed(&error),
    };
    let hits = found.into_iter().map(|person| (person, None)).collect();
    answer(operator, &parts, "search/identifier", &bytes, window, hits).await
}

/// Answers `POST {base}/operator/search/demographics`.
pub async fn by_demographics(operator: &Operator, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let (search, bytes) = match wire::posted::<DemographicsSearch>(&parts, body).await {
        Ok(read) => read,
        Err(refused) => return *refused,
    };
    let Some(input) = input(&search) else {
        return wire::refuse(StatusCode::BAD_REQUEST, IssueType::Invalid, NO_FIELD);
    };
    let window = Window::of(search.offset, search.limit);
    let matched = match matched(operator, &input).await {
        Ok(matched) => matched,
        Err(error) => return wire::failed(&error),
    };
    let hits = matched
        .into_iter()
        .map(|found| (found.person, Some((found.score, found.grade))))
        .collect();
    answer(
        operator,
        &parts,
        "search/demographics",
        &bytes,
        window,
        hits,
    )
    .await
}

/// Reads the persons `search` names, or `None` when its domain is unknown
/// or retired.
async fn identifier_persons(
    operator: &Operator,
    search: &IdentifierSearch,
) -> Result<Option<Vec<Uuid>>, OperatorError> {
    let mut connection = operator.store.pool().acquire().await?;
    let Some(domain) = record::domain(&mut *connection, &search.system).await? else {
        return Ok(None);
    };
    if domain.retired {
        return Ok(None);
    }
    // NOTE: PIXm 3.1.0 §2:3.83.4.2.2.2; a value its domain's rule refuses
    // names no record the store can hold, so it finds no person.
    let Ok(value) = Rule::of(&search.system).normalise(&search.value) else {
        return Ok(Some(Vec::new()));
    };
    let hmac = operator.store.sealer().hmac(domain.id.uuid(), &value);
    Ok(Some(
        xref::persons(&mut connection, domain.id.uuid(), &hmac).await?,
    ))
}

/// Returns the `Patient` the scorer reads `search` as, or `None` when it
/// states no field.
fn input(search: &DemographicsSearch) -> Option<Patient> {
    let stated = |text: &Option<String>| {
        text.as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let family = stated(&search.family);
    let given = stated(&search.given);
    let birth_date = stated(&search.birth_date);
    let gender = stated(&search.gender);
    let postal_code = stated(&search.postal_code);
    if family.is_none()
        && given.is_none()
        && birth_date.is_none()
        && gender.is_none()
        && postal_code.is_none()
    {
        return None;
    }
    let name = (family.is_some() || given.is_some()).then(|| HumanName {
        family: family.map(Into::into),
        given: given.into_iter().map(Into::into).collect(),
        ..HumanName::default()
    });
    let address = postal_code.map(|code| Address {
        postal_code: Some(code.into()),
        ..Address::default()
    });
    Some(Patient {
        name: name.into_iter().collect(),
        birth_date: birth_date.map(Into::into),
        gender: gender.map(Into::into),
        address: address.into_iter().collect(),
        ..Patient::default()
    })
}

/// Runs the scorer over `input` in one snapshot.
async fn matched(
    operator: &Operator,
    input: &Patient,
) -> Result<Vec<matching::Found>, OperatorError> {
    let mut snapshot = snapshot(&operator.store).await?;
    let matched = matching::run(
        &mut snapshot,
        operator.store.sealer(),
        &operator.scorer,
        &operator.blocking,
        input,
    )
    .await?;
    snapshot.commit().await?;
    Ok(matched.found)
}

/// Writes the search's audit records and answers its page of `hits`, each
/// a person with its score and grade for a demographics search.
async fn answer(
    operator: &Operator,
    parts: &Parts,
    rest: &str,
    bytes: &[u8],
    window: Window,
    hits: Vec<(Uuid, Option<(f64, Grade)>)>,
) -> Response {
    let (total, page, next) = window.page(hits);
    let persons = match read_hits(operator, &page).await {
        Ok(persons) => persons,
        Err(error) => return wire::failed(&error),
    };
    let references: Vec<String> = page
        .iter()
        .map(|(person, _)| format!("Patient/{person}"))
        .collect();
    let call = wire::call(operator, parts);
    let exchanges = records::steward_search(
        &call,
        &raw(operator, rest, bytes),
        &references,
        Outcome::Success,
    );
    if let Err(error) = wire::record(operator, parts, exchanges).await {
        return wire::unaudited(&error);
    }
    wire::answer(&Results {
        total,
        offset: window.offset,
        next,
        persons,
    })
}

/// Reads each person of `page` as a hit, in order.
async fn read_hits(
    operator: &Operator,
    page: &[(Uuid, Option<(f64, Grade)>)],
) -> Result<Vec<Hit>, OperatorError> {
    let mut snapshot = snapshot(&operator.store).await?;
    let domains = domains(&mut snapshot).await?;
    let mut hits = Vec::with_capacity(page.len());
    for (person, scored) in page {
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM pix.person WHERE id = $1")
                .bind(person)
                .fetch_optional(&mut *snapshot)
                .await?;
        let status = match status.as_deref() {
            Some("active") => PersonStatus::Active,
            Some("merged") => PersonStatus::Merged,
            _ => return Err(OperatorError::Unadmitted("person status")),
        };
        let identifiers = xref::identifiers(&mut snapshot, operator.store.sealer(), *person)
            .await?
            .into_iter()
            .map(|(system, value)| {
                let domain = domains
                    .get(&system)
                    .cloned()
                    .ok_or(OperatorError::Unadmitted("identifier domain"))?;
                Ok(MaskedIdentifier {
                    domain,
                    masked: Masked(mask(value.expose())),
                })
            })
            .collect::<Result<Vec<_>, OperatorError>>()?;
        hits.push(Hit {
            id: person.to_string(),
            status,
            leading: leading(&mut snapshot, *person).await?,
            identifiers,
            score: scored.map(|(score, _)| score),
            grade: scored.map(|(_, grade)| grade_of(grade)),
        });
    }
    snapshot.commit().await?;
    Ok(hits)
}

/// Reads every domain as a list row names it, by system.
async fn domains(
    connection: &mut PgConnection,
) -> Result<BTreeMap<String, DomainRef>, OperatorError> {
    let rows: Vec<(Uuid, String, String, String)> =
        sqlx::query_as("SELECT id, system, name, kind FROM pix.identifier_domain")
            .fetch_all(&mut *connection)
            .await?;
    rows.into_iter()
        .map(|(id, system, name, kind)| {
            Ok((
                system.clone(),
                DomainRef {
                    id: id.to_string(),
                    system,
                    name,
                    kind: domain_kind(&kind)?,
                },
            ))
        })
        .collect()
}

/// Returns the document's grade of a scorer's `grade`.
const fn grade_of(grade: Grade) -> ferropix_operator_api::patient::Grade {
    match grade {
        Grade::Certain => ferropix_operator_api::patient::Grade::Certain,
        Grade::Probable => ferropix_operator_api::patient::Grade::Probable,
        Grade::Possible => ferropix_operator_api::patient::Grade::Possible,
        Grade::CertainlyNot => ferropix_operator_api::patient::Grade::CertainlyNot,
    }
}

/// Returns the raw request the BALP records carry: the request line under
/// the operator API's base, the media type and the body.
fn raw(operator: &Operator, rest: &str, bytes: &[u8]) -> SecretString {
    let base = operator.endpoint.as_str().trim_end_matches('/');
    let url =
        url::Url::parse(&format!("{base}/{rest}")).unwrap_or_else(|_| operator.endpoint.clone());
    let body = SecretString::from(String::from_utf8_lossy(bytes).into_owned());
    records::raw_request(&url, Some((wire::JSON, &body)))
}

#[cfg(test)]
mod tests {
    use ferropix_operator_api::search::DemographicsSearch;

    use super::{Window, input};

    #[test]
    fn a_window_bounds_its_limit_and_names_the_next_offset_while_persons_follow() {
        let window = Window::of(Some(2), Some(1_000));
        assert_eq!(50, window.limit);
        let (total, page, next) = Window::of(Some(1), Some(2)).page(vec![1, 2, 3, 4]);
        assert_eq!((4, vec![2, 3], Some(3)), (total, page, next));
        let (_, last, next) = Window::of(Some(2), Some(2)).page(vec![1, 2, 3, 4]);
        assert_eq!((vec![3, 4], None), (last, next));
        assert_eq!(1, Window::of(None, Some(0)).limit);
    }

    #[test]
    fn a_search_of_blank_fields_states_nothing_and_a_family_name_is_a_patient_name() {
        let blank = DemographicsSearch {
            family: Some(String::from("  ")),
            ..DemographicsSearch::default()
        };
        assert!(input(&blank).is_none());
        let family = ferropix_testkit::synthetic::FAMILY_NAMES[0];
        let patient = input(&DemographicsSearch {
            family: Some(family.to_owned()),
            postal_code: Some(String::from("9999 ZZ")),
            ..DemographicsSearch::default()
        })
        .expect("a field is stated");
        assert_eq!(
            Some(family),
            patient.name[0]
                .family
                .as_ref()
                .and_then(|name| name.value.as_deref())
        );
        assert_eq!(1, patient.address.len());
    }
}
