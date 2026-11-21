// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-78 search over the store: one statement that counts the persons
//! matching every criterion and reads one page of them.
//!
//! Each criterion holds (AND) and each of its values is an alternative (OR)
//! (FHIR R4 search, "Combining"; PDQm 3.2.0 §2:3.78.4.1.2.1). A string
//! parameter matches a field that starts with the value once both are
//! folded to their base letters and lowercased, and `:exact` matches the
//! field as written (§2:3.78.4.1.2.2; FHIR R4 search, string). An
//! `identifier` is found by the HMAC of its normalised value in its
//! domain, so no sealed value is opened to search (`docs/architecture.md`
//! §4.2). A `birthdate` compares the ranges of days the value and the
//! stored date cover (FHIR R4 search, date). The demographics searched are
//! those the answer shows ([`crate::pdqm::patient`]), read from the served
//! row and the search keys `pix.serve` keeps for each person (#722), so a
//! criterion reads an index; a merged person holds no record, so only `_id`
//! and `active` find it. The page is the persons
//! after a cursor in id order, so a page does not shift when a person is
//! created or merged while a client pages (our own design).

use std::fmt;

use fhir_search::date::SearchDate;
use fhir_search::prefix::Prefix;
use ihe_iti::pdqm::query::{Gender, StringMatch};
use ihe_iti::pdqm::server::search::{Criterion, PatientSearch, StringParameter, Token};
use jiff::ToSpan as _;
use jiff::civil::Date;
use secrecy::ExposeSecret as _;
use sqlx::{PgConnection, Postgres, QueryBuilder};
use uuid::Uuid;

use crate::matching::subject::MOTHERS_MAIDEN_NAME;
use crate::pdqm::PdqError;
use crate::store::identifier::Rule;
use crate::store::record::{self, Domain};
use crate::store::seal::Sealer;

#[cfg(test)]
mod oracle;

/// One page of a search's result set.
#[derive(Clone, PartialEq, Eq)]
pub struct Page {
    /// Every person matching the search.
    pub total: u32,
    /// The persons of this page, in id order.
    pub persons: Vec<Uuid>,
    /// Whether persons follow this page.
    pub more: bool,
}

impl fmt::Debug for Page {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Page")
            .field("total", &self.total)
            .field("persons", &self.persons.len())
            .field("more", &self.more)
            .finish()
    }
}

/// What one search reads beside its criteria.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    /// The persons a page holds at most.
    pub size: u32,
    /// The last person of the page before, `None` for the first page.
    pub after: Option<Uuid>,
    /// Whether merged persons are left out (`[pdqm] deprecated = "omit"`).
    pub omit_merged: bool,
}

/// Runs `search` over the store and returns the page `window` names.
///
/// `master` is the system of the Master Patient Identification Domain,
/// whose values are person ids, and `keys` give each identifier's HMAC.
///
/// # Errors
/// Returns [`PdqError::Store`] when a statement fails.
pub async fn run(
    connection: &mut PgConnection,
    keys: &Sealer,
    master: &str,
    search: &PatientSearch,
    window: Window,
) -> Result<Page, PdqError> {
    let domains = record::domains(&mut *connection)
        .await?
        .into_iter()
        .filter(|domain| !domain.retired)
        .collect::<Vec<_>>();
    let mut query: QueryBuilder<Postgres> = QueryBuilder::new(
        "WITH matched AS (
             SELECT p.id FROM pix.person p
             LEFT JOIN pix.person_served s ON s.person_id = p.id
             WHERE true",
    );
    if window.omit_merged {
        query.push(" AND p.status = 'active'");
    }
    for criterion in search.criteria() {
        query.push(" AND (");
        push_criterion(&mut query, criterion, keys, master, &domains)?;
        query.push(")");
    }
    page(connection, query, window).await
}

/// Ends `query`, whose `matched` names the persons a search matches, with
/// the count and the page `window` names, and runs it.
///
/// # Errors
/// Returns [`PdqError::Store`] when the statement fails.
pub(super) async fn page(
    connection: &mut PgConnection,
    mut query: QueryBuilder<'_, Postgres>,
    window: Window,
) -> Result<Page, PdqError> {
    query.push(
        ")
         SELECT (SELECT count(*) FROM matched),
                ARRAY(SELECT id FROM matched WHERE ",
    );
    match window.after {
        Some(after) => {
            query.push("id > ");
            query.push_bind(after);
        }
        None => {
            query.push("true");
        }
    }
    query.push(" ORDER BY id LIMIT ");
    query.push_bind(i64::from(window.size).saturating_add(1));
    query.push(")");
    let (total, mut persons): (i64, Vec<Uuid>) = query
        .build_query_as()
        .fetch_one(&mut *connection)
        .await
        .map_err(|source| PdqError::Store(source.into()))?;
    let size = usize::try_from(window.size).unwrap_or(usize::MAX);
    let more = persons.len() > size;
    persons.truncate(size);
    Ok(Page {
        total: u32::try_from(total).unwrap_or(u32::MAX),
        persons,
        more,
    })
}

/// The served birth range a `birthdate` criterion compares.
const BIRTH: &str = "s.birth";

/// Writes the condition of one criterion.
///
/// # Errors
/// Returns [`PdqError::Unsupported`] for a criterion, modifier, prefix or
/// code this build of the search does not read.
fn push_criterion(
    query: &mut QueryBuilder<Postgres>,
    criterion: &Criterion,
    keys: &Sealer,
    master: &str,
    domains: &[Domain],
) -> Result<(), PdqError> {
    match criterion {
        Criterion::Id(values) => {
            // NOTE: PDQm 3.2.0 §2:3.78.4.1.2.1, `_id` is an exact match; an id
            // that is no uuid names no person this store holds.
            let ids: Vec<Uuid> = values
                .iter()
                .filter_map(|value| Uuid::try_parse(value.expose_secret()).ok())
                .collect();
            query.push("p.id = ANY(");
            query.push_bind(ids);
            query.push(")");
        }
        Criterion::Active(values) => {
            query.push("(p.status = 'active') = ANY(");
            query.push_bind(values.clone());
            query.push(")");
        }
        Criterion::Text {
            parameter,
            matching,
            values,
        } => {
            let values: Vec<String> = values
                .iter()
                .map(|value| value.expose_secret().to_owned())
                .collect();
            let field = field(*parameter)?;
            match matching {
                StringMatch::Exact => {
                    query.push(
                        "p.id IN (SELECT k.person_id FROM pix.person_search_key k WHERE k.field = ",
                    );
                    query.push_bind(field);
                    query.push("::\"char\" AND k.value = ANY(");
                    query.push_bind(values);
                    query.push(")");
                }
                StringMatch::StartsWith => {
                    query.push("p.id IN (SELECT k.person_id FROM unnest(");
                    query.push_bind(values);
                    query.push(
                        "::text[]) AS v(value) JOIN pix.person_search_key k ON k.folded ^@ pix.folded_key(",
                    );
                    query.push_bind(field);
                    query.push("::\"char\", v.value) WHERE true");
                }
                _ => return Err(PdqError::Unsupported),
            }
            if matches!(parameter, StringParameter::MothersMaidenName) {
                query.push(" AND k.system = ");
                query.push_bind(MOTHERS_MAIDEN_NAME);
            }
            query.push(")");
        }
        Criterion::Identifier(tokens) => push_identifiers(query, tokens, keys, master, domains),
        Criterion::Telecom(tokens) => push_telecom(query, tokens),
        Criterion::Birthdate(values) => {
            query.push("false");
            for value in values {
                if let Some((low, high)) = range(value.date().expose_secret()) {
                    query.push(" OR ");
                    push_date(query, BIRTH, value.prefix(), low, high);
                }
            }
        }
        Criterion::Gender(gender) => {
            query.push("s.gender = ");
            query.push_bind(gender_code(*gender)?);
        }
        _ => return Err(PdqError::Unsupported),
    }
    Ok(())
}

/// Returns the field of `parameter`'s search keys (`pix.person_search_key`).
///
/// # Errors
/// Returns [`PdqError::Unsupported`] for a parameter this build does not
/// read.
fn field(parameter: StringParameter) -> Result<&'static str, PdqError> {
    match parameter {
        StringParameter::Family => Ok("f"),
        StringParameter::Given => Ok("g"),
        StringParameter::Address => Ok("a"),
        StringParameter::AddressCity => Ok("c"),
        StringParameter::AddressCountry => Ok("n"),
        StringParameter::AddressPostalcode => Ok("p"),
        StringParameter::AddressState => Ok("s"),
        StringParameter::MothersMaidenName => Ok("x"),
        _ => Err(PdqError::Unsupported),
    }
}

/// Writes the condition of an `identifier` criterion: a person one of whose
/// active records holds one of `tokens`, or carries it in an
/// `authoritative` domain where no record holds it, as a PIX query finds
/// it ([`crate::store::xref`]), each found by the HMAC of the value its
/// domain normalises; or whose master identifier is one of them.
pub(super) fn push_identifiers(
    query: &mut QueryBuilder<Postgres>,
    tokens: &[Token],
    keys: &Sealer,
    master: &str,
    domains: &[Domain],
) {
    let mut domain_ids: Vec<Uuid> = Vec::new();
    let mut hmacs: Vec<Vec<u8>> = Vec::new();
    let mut persons: Vec<Uuid> = Vec::new();
    for token in tokens {
        let value = token.value().expose_secret();
        let named = |domain: &&Domain| match token.system() {
            None => true,
            Some(system) => domain.system == system,
        };
        for domain in domains.iter().filter(named) {
            // NOTE: PDQm 3.2.0 §2:3.78.4.1.3 Case 3; a value its domain's rule
            // refuses names no record the store can hold, so it matches none.
            if let Ok(normalised) = Rule::of(&domain.system).normalise(value) {
                domain_ids.push(domain.id.uuid());
                hmacs.push(keys.hmac(domain.id.uuid(), &normalised).to_vec());
            }
        }
        if token.system().is_none_or(|system| system == master)
            && let Ok(person) = Uuid::try_parse(value)
        {
            persons.push(person);
        }
    }
    query.push("p.id = ANY(");
    query.push_bind(persons);
    query.push(
        ") OR p.id IN (
             SELECT l.person_id FROM pix.source_record r
             JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
             WHERE (r.domain_id, r.identifier_hmac) IN (SELECT * FROM unnest(",
    );
    query.push_bind(domain_ids.clone());
    query.push("::uuid[], ");
    query.push_bind(hmacs.clone());
    query.push(
        "::bytea[])) AND r.active AND r.removed_at IS NULL)
         OR p.id IN (
             SELECT l.person_id FROM pix.carried_identifier c
             JOIN pix.identifier_domain d ON d.id = c.domain_id
             JOIN pix.source_record r ON r.id = c.record_id AND r.head_version = c.version
             JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
             WHERE (c.domain_id, c.identifier_hmac) IN (SELECT * FROM unnest(",
    );
    query.push_bind(domain_ids);
    query.push("::uuid[], ");
    query.push_bind(hmacs);
    query.push(
        "::bytea[])) AND d.kind = 'authoritative' AND r.active AND r.removed_at IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM pix.source_record o
                   WHERE o.domain_id = c.domain_id AND o.identifier_hmac = c.identifier_hmac
               ))",
    );
}

/// Writes the condition of a `telecom` criterion: a contact point whose
/// value is one of `tokens`, in the token's `ContactPoint.system` when it
/// names one (FHIR R4 search, token on a `ContactPoint`).
fn push_telecom(query: &mut QueryBuilder<Postgres>, tokens: &[Token]) {
    query.push(
        "p.id IN (SELECT k.person_id FROM pix.person_search_key k WHERE k.field = 't' AND (false",
    );
    for token in tokens {
        query.push(" OR (k.value = ");
        query.push_bind(token.value().expose_secret().to_owned());
        match token.system() {
            None => {}
            Some("") => {
                query.push(" AND k.system IS NULL");
            }
            Some(system) => {
                query.push(" AND k.system = ");
                query.push_bind(system.to_owned());
            }
        }
        query.push(")");
    }
    query.push("))");
}

/// Writes the comparison of the stored birth date with the days
/// `[low, high)` a search value covers, as `prefix` asks (FHIR R4 search,
/// prefixes).
pub(super) fn push_date(
    query: &mut QueryBuilder<Postgres>,
    stored: &str,
    prefix: Prefix,
    low: Date,
    high: Date,
) {
    let mut bind = |query: &mut QueryBuilder<Postgres>, date: Date| {
        query.push_bind(date.to_string());
        query.push("::date");
    };
    let within = |query: &mut QueryBuilder<Postgres>,
                  bind: &mut dyn FnMut(&mut QueryBuilder<Postgres>, Date)| {
        query.push(format!("{stored} <@ daterange("));
        bind(query, low);
        query.push(", ");
        bind(query, high);
        query.push(")");
    };
    match prefix {
        Prefix::Ne => {
            query.push("NOT (");
            within(query, &mut bind);
            query.push(")");
        }
        Prefix::Gt | Prefix::Ge => {
            query.push(format!("(upper({stored}) > "));
            bind(query, high);
            if prefix == Prefix::Ge {
                query.push(" OR ");
                within(query, &mut bind);
            }
            query.push(")");
        }
        Prefix::Lt | Prefix::Le => {
            query.push(format!("(lower({stored}) < "));
            bind(query, low);
            if prefix == Prefix::Le {
                query.push(" OR ");
                within(query, &mut bind);
            }
            query.push(")");
        }
        Prefix::Sa => {
            query.push(format!("lower({stored}) >= "));
            bind(query, high);
        }
        Prefix::Eb => {
            query.push(format!("upper({stored}) <= "));
            bind(query, low);
        }
        Prefix::Ap => {
            let (low, high) = approximately(low, high);
            query.push(format!("{stored} && daterange("));
            bind(query, low);
            query.push(", ");
            bind(query, high);
            query.push(")");
        }
        Prefix::Eq => within(query, &mut bind),
    }
}

/// Returns the days `[low, high)` the FHIR `date` `text` covers at its
/// precision: a year, a month or a day, read by the FHIR search grammar
/// (FHIR R4 search.html §3.1.1.4.7). `None` when the range runs past the
/// year 9999, which no birth date reaches.
pub(super) fn range(text: &str) -> Option<(Date, Date)> {
    // NOTE: no specification governs this: our own design; the reader held the
    // value to a FHIR date, so a value that does not read here is no birth date.
    let range = SearchDate::parse(text).ok()?.range();
    Some((range.low().first_day(), range.high()?.first_day()))
}

/// Returns `[low, high)` widened on each side by a tenth of the days
/// between `low` and today, at least one day, the approximation FHIR R4
/// recommends for `ap` (FHIR R4 search, prefixes).
fn approximately(low: Date, high: Date) -> (Date, Date) {
    let today = jiff::Zoned::now().date();
    let days = today
        .since(low)
        .map_or(0, |span| span.get_days().unsigned_abs());
    let margin = i64::from(days.checked_div(10).unwrap_or(0).max(1));
    let widened = |date: Date, sign: i64| {
        date.checked_add(margin.saturating_mul(sign).days())
            .unwrap_or(date)
    };
    (widened(low, -1), widened(high, 1))
}

/// Returns the `administrative-gender` code of `gender`.
///
/// # Errors
/// Returns [`PdqError::Unsupported`] for a gender this build does not know.
pub(super) fn gender_code(gender: Gender) -> Result<&'static str, PdqError> {
    match gender {
        Gender::Male => Ok("male"),
        Gender::Female => Ok("female"),
        Gender::Other => Ok("other"),
        Gender::Unknown => Ok("unknown"),
        _ => Err(PdqError::Unsupported),
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::{approximately, range};

    #[test]
    fn a_date_covers_the_days_of_its_precision() {
        assert_eq!(Some((date(1961, 1, 1), date(1962, 1, 1))), range("1961"));
        assert_eq!(Some((date(1961, 4, 1), date(1961, 5, 1))), range("1961-04"));
        assert_eq!(
            Some((date(1961, 4, 12), date(1961, 4, 13))),
            range("1961-04-12")
        );
        assert_eq!(None, range("1961-02-30"));
        assert_eq!(None, range("x"));
    }

    #[test]
    fn an_approximate_date_widens_by_at_least_a_day() {
        let (low, high) = approximately(date(1961, 4, 12), date(1961, 4, 13));
        assert!(low < date(1961, 4, 12), "{low}");
        assert!(high > date(1961, 4, 13), "{high}");
    }
}
