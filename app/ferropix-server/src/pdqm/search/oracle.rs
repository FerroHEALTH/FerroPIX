// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-78 search as it read before the search keys (#722): every
//! criterion over the demographics each person serves, built for every
//! person on each search. It is kept only as the oracle the search over the
//! keys must agree with, page for page, over a seeded corpus.

use ihe_iti::pdqm::query::StringMatch;
use ihe_iti::pdqm::server::search::{Criterion, PatientSearch, StringParameter, Token};
use secrecy::ExposeSecret as _;
use sqlx::{PgConnection, Postgres, QueryBuilder};
use uuid::Uuid;

use super::{Page, Window, gender_code, page, push_date, push_identifiers, range};
use crate::matching::subject::MOTHERS_MAIDEN_NAME;
use crate::pdqm::PdqError;
use crate::pdqm::patient::{SERVED, SERVED_ORDER};
use crate::store::record::{self, Domain};
use crate::store::seal::Sealer;

/// Runs `search` as the search read before the keys.
pub(crate) async fn run(
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
    let mut query: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "WITH served AS ({SERVED} {SERVED_ORDER}),
         matched AS (
             SELECT p.id FROM pix.person p LEFT JOIN served s ON s.person_id = p.id
             WHERE true"
    ));
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

fn push_criterion(
    query: &mut QueryBuilder<Postgres>,
    criterion: &Criterion,
    keys: &Sealer,
    master: &str,
    domains: &[Domain],
) -> Result<(), PdqError> {
    match criterion {
        Criterion::Id(values) => {
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
            query.push("EXISTS (SELECT 1 FROM (");
            push_fields(query, *parameter)?;
            query.push(") AS f(field) WHERE ");
            match matching {
                StringMatch::Exact => {
                    query.push("f.field = ANY(");
                    query.push_bind(values);
                    query.push(")");
                }
                StringMatch::StartsWith => {
                    query.push("EXISTS (SELECT 1 FROM unnest(");
                    query.push_bind(values);
                    query.push(
                        "::text[]) AS v(value) WHERE starts_with(pix.fold(f.field), pix.fold(v.value)))",
                    );
                }
                _ => return Err(PdqError::Unsupported),
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
                    push_date(
                        query,
                        "pix.birth_range(s.demographics->>'birthDate')",
                        value.prefix(),
                        low,
                        high,
                    );
                }
            }
        }
        Criterion::Gender(gender) => {
            query.push("s.demographics->>'gender' = ");
            query.push_bind(gender_code(*gender)?);
        }
        _ => return Err(PdqError::Unsupported),
    }
    Ok(())
}

fn push_fields(
    query: &mut QueryBuilder<Postgres>,
    parameter: StringParameter,
) -> Result<(), PdqError> {
    let address =
        |part: &str| format!("SELECT a->>'{part}' FROM pix.elements(s.demographics->'address') a");
    let select = match parameter {
        StringParameter::Family => {
            String::from("SELECT n->>'family' FROM pix.elements(s.demographics->'name') n")
        }
        StringParameter::Given => String::from(
            "SELECT g FROM pix.elements(s.demographics->'name') n, pix.texts(n->'given') g",
        ),
        StringParameter::Address => String::from(
            "SELECT part FROM pix.elements(s.demographics->'address') a,
                 LATERAL (SELECT a->>'text' UNION ALL SELECT a->>'city'
                          UNION ALL SELECT a->>'district' UNION ALL SELECT a->>'state'
                          UNION ALL SELECT a->>'postalCode' UNION ALL SELECT a->>'country'
                          UNION ALL SELECT pix.texts(a->'line')) AS parts(part)",
        ),
        StringParameter::AddressCity => address("city"),
        StringParameter::AddressCountry => address("country"),
        StringParameter::AddressPostalcode => address("postalCode"),
        StringParameter::AddressState => address("state"),
        StringParameter::MothersMaidenName => {
            query.push(
                "SELECT e->>'valueString' FROM pix.elements(s.demographics->'extension') e
                 WHERE e->>'url' = ",
            );
            query.push_bind(MOTHERS_MAIDEN_NAME);
            return Ok(());
        }
        _ => return Err(PdqError::Unsupported),
    };
    query.push(select);
    Ok(())
}

fn push_telecom(query: &mut QueryBuilder<Postgres>, tokens: &[Token]) {
    query.push("EXISTS (SELECT 1 FROM pix.elements(s.demographics->'telecom') t WHERE false");
    for token in tokens {
        query.push(" OR (t->>'value' = ");
        query.push_bind(token.value().expose_secret().to_owned());
        match token.system() {
            None => {}
            Some("") => {
                query.push(" AND t->>'system' IS NULL");
            }
            Some(system) => {
                query.push(" AND t->>'system' = ");
                query.push_bind(system.to_owned());
            }
        }
        query.push(")");
    }
    query.push(")");
}
