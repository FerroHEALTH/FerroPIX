// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pseudonymisation domains and the pseudonyms.
//!
//! - `GET {base}/operator/pseudonym-domains` lists every domain with its
//!   count, retired ones included; it names no person, so it writes no
//!   audit record.
//! - Behind the administrator scope, `POST {base}/operator/pseudonym-domains`
//!   adds a domain whose namespace no identifier domain holds, and `PATCH
//!   {base}/operator/pseudonym-domains/{id}` renames one, names another
//!   consumer or retires it; a retired domain stays retired. Each write is a
//!   `pseudonym-domain-create` or `pseudonym-domain-update` steward action
//!   with its record, in one transaction.
//! - `POST {base}/operator/pseudonyms` mints a person's pseudonym in a
//!   domain when its survivor has none: a `pseudonym-mint` action and a BALP
//!   Create record, and the answer says whether it minted, never the value.
//! - `POST {base}/operator/pseudonyms/resolve` answers the person a
//!   pseudonym resolves to, through the merge chain, with the steward's
//!   reason: a `pseudonym-resolve` action and a BALP disclosure record
//!   (BALP 1.1.4 §3:5.7.7), committed before the answer leaves.
//!
//! No log line, span or error carries a pseudonym or a reason. No
//! specification governs these routes: our own design.

use std::sync::Arc;

use axum::extract::Request;
use axum::response::Response;
use axum::routing::{MethodFilter, MethodRouter, on};
use ferropix_operator_api::pseudonym::{
    MAX_REASON, Mint, Minted, NewPseudonymDomain, PseudonymDomain, PseudonymDomainChange,
    PseudonymDomains, Resolution, Resolve,
};
use http::{Method, StatusCode};
use ihe_atna::balp::Outcome;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::audit::records::{self, operator::Disclosed};
use crate::auth::table::Route;
use crate::config::section::domain::is_system;
use crate::fhir::outcome::IssueType;
use crate::operator::{Operator, OperatorError, Recorded, count, record_action, wire};
use crate::store::pseudonym;
use crate::store::record::PersonId;

/// The diagnostics of an id that names no pseudonymisation domain.
pub const NO_SUCH_DOMAIN: &str = "no pseudonymisation domain has this id";

/// The diagnostics of a domain write whose fields do not hold.
pub const NOT_A_DOMAIN: &str = "a pseudonymisation domain names an absolute URI as its namespace and a name, and a consumer is no blank";

/// The diagnostics of a namespace another domain holds.
pub const NAMESPACE_TAKEN: &str =
    "another pseudonymisation domain or an identifier domain holds this namespace";

/// The diagnostics of a change to, or a mint in, a retired domain.
pub const RETIRED: &str = "the pseudonymisation domain is retired: it mints nothing";

/// The diagnostics of a write or a resolution without a reason.
pub const NO_REASON: &str = "the request states its reason, in at most 500 characters";

/// The diagnostics of a person id that names no person.
pub const NO_SUCH_PERSON: &str = "no person has this id";

/// The diagnostics of a pseudonym that resolves to no person.
pub const NOT_RESOLVED: &str = "no unretired pseudonymisation domain holds this pseudonym";

/// The columns of one domain row, each timestamp RFC 3339 UTC.
const COLUMNS: &str = "d.id, d.namespace, d.name, d.consumer_client_id,
     to_char(d.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS created_at,
     to_char(d.retired_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS retired_at,
     (SELECT count(*) FROM pix.pseudonym p WHERE p.domain_id = d.id) AS pseudonyms";

/// One domain as the store holds it.
#[derive(sqlx::FromRow)]
struct DomainRow {
    id: Uuid,
    namespace: String,
    name: String,
    consumer_client_id: Option<String>,
    created_at: String,
    retired_at: Option<String>,
    pseudonyms: i64,
}

impl From<DomainRow> for PseudonymDomain {
    fn from(row: DomainRow) -> Self {
        Self {
            id: row.id.to_string(),
            namespace: row.namespace,
            name: row.name,
            consumer_client_id: row.consumer_client_id,
            created_at: row.created_at,
            retired_at: row.retired_at,
            pseudonyms: count(row.pseudonyms),
        }
    }
}

/// Returns the handler of `route` when it is a route of this module, under
/// `filter`.
#[must_use]
pub fn handler(
    route: &Route,
    filter: MethodFilter,
    operator: &Arc<Operator>,
) -> Option<MethodRouter> {
    let operator = Arc::clone(operator);
    let handler = match (route.method.clone(), route.path) {
        (Method::GET, "/operator/pseudonym-domains") => on(filter, move || {
            let operator = Arc::clone(&operator);
            async move { list(&operator).await }
        }),
        (Method::POST, "/operator/pseudonym-domains") => on(filter, move |request: Request| {
            let operator = Arc::clone(&operator);
            async move { create(&operator, request).await }
        }),
        (Method::PATCH, "/operator/pseudonym-domains/{id}") => {
            on(filter, move |request: Request| {
                let operator = Arc::clone(&operator);
                async move { update(&operator, request).await }
            })
        }
        (Method::POST, "/operator/pseudonyms") => on(filter, move |request: Request| {
            let operator = Arc::clone(&operator);
            async move { mint(&operator, request).await }
        }),
        (Method::POST, "/operator/pseudonyms/resolve") => on(filter, move |request: Request| {
            let operator = Arc::clone(&operator);
            async move { resolve(&operator, request).await }
        }),
        _ => return None,
    };
    Some(handler)
}

/// Answers the listing of the pseudonymisation domains.
pub async fn list(operator: &Operator) -> Response {
    let rows: Result<Vec<DomainRow>, sqlx::Error> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM pix.pseudonym_domain d ORDER BY d.namespace"
    )))
    .fetch_all(operator.store.pool())
    .await;
    match rows {
        Ok(rows) => wire::answer(&PseudonymDomains {
            domains: rows.into_iter().map(PseudonymDomain::from).collect(),
        }),
        Err(error) => wire::failed(&error.into()),
    }
}

/// Reads the domain `id`, `None` when the store holds none.
async fn read_one(operator: &Operator, id: Uuid) -> Result<Option<PseudonymDomain>, OperatorError> {
    let row: Option<DomainRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM pix.pseudonym_domain d WHERE d.id = $1"
    )))
    .bind(id)
    .fetch_optional(operator.store.pool())
    .await?;
    Ok(row.map(PseudonymDomain::from))
}

/// Returns whether `text` is a written reason within the bound.
fn reasoned(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && text.chars().count() <= MAX_REASON
}

/// Answers `POST {base}/operator/pseudonym-domains`.
pub async fn create(operator: &Operator, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let (new, _bytes) = match wire::posted::<NewPseudonymDomain>(&parts, body).await {
        Ok(read) => read,
        Err(refused) => return *refused,
    };
    let blank_consumer = new
        .consumer_client_id
        .as_deref()
        .is_some_and(|client| client.trim().is_empty());
    if !is_system(&new.namespace) || new.name.trim().is_empty() || blank_consumer {
        return wire::refuse(StatusCode::BAD_REQUEST, IssueType::Invalid, NOT_A_DOMAIN);
    }
    if new
        .reason
        .as_deref()
        .is_some_and(|reason| !reasoned(reason))
    {
        return wire::refuse(StatusCode::BAD_REQUEST, IssueType::Invalid, NO_REASON);
    }
    let outcome = async {
        let mut transaction = operator.store.pool().begin().await?;
        let id: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO pix.pseudonym_domain (namespace, name, consumer_client_id)
             SELECT $1, $2, $3
             WHERE NOT EXISTS (SELECT 1 FROM pix.identifier_domain WHERE system = $1)
             ON CONFLICT (namespace) DO NOTHING
             RETURNING id",
        )
        .bind(&new.namespace)
        .bind(new.name.trim())
        .bind(new.consumer_client_id.as_deref().map(str::trim))
        .fetch_optional(&mut *transaction)
        .await?;
        Ok::<_, OperatorError>(id.map(|id| (transaction, id)))
    }
    .await;
    let (transaction, id) = match outcome {
        Ok(Some(written)) => written,
        Ok(None) => {
            return wire::refuse(StatusCode::CONFLICT, IssueType::Conflict, NAMESPACE_TAKEN);
        }
        Err(error) => return wire::failed(&error),
    };
    let write = Write {
        created: true,
        namespace: &new.namespace,
        id,
        reason: new.reason.as_deref(),
    };
    match finish(operator, &parts, transaction, &write).await {
        Ok(domain) => crate::fhir::outcome::body(StatusCode::CREATED, &domain, wire::JSON),
        Err(refused) => *refused,
    }
}

/// Answers `PATCH {base}/operator/pseudonym-domains/{id}`.
pub async fn update(operator: &Operator, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let Some(id) = wire::path_id(&parts) else {
        return no_such_domain();
    };
    let (change, _bytes) = match wire::posted::<PseudonymDomainChange>(&parts, body).await {
        Ok(read) => read,
        Err(refused) => return *refused,
    };
    if !reasoned(&change.reason) {
        return wire::refuse(StatusCode::BAD_REQUEST, IssueType::Invalid, NO_REASON);
    }
    let blank = |text: &Option<String>| text.as_deref().is_some_and(|text| text.trim().is_empty());
    if blank(&change.name) || blank(&change.consumer_client_id) {
        return wire::refuse(StatusCode::BAD_REQUEST, IssueType::Invalid, NOT_A_DOMAIN);
    }
    let outcome = async {
        let mut transaction = operator.store.pool().begin().await?;
        let current: Option<(String, bool)> = sqlx::query_as(
            "SELECT namespace, retired_at IS NOT NULL FROM pix.pseudonym_domain
             WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((namespace, retired)) = current else {
            return Ok(Changed::Missing);
        };
        if retired {
            return Ok(Changed::Retired);
        }
        sqlx::query(
            "UPDATE pix.pseudonym_domain
             SET name = coalesce($2, name),
                 consumer_client_id = coalesce($3, consumer_client_id),
                 retired_at = CASE WHEN $4 THEN now() ELSE retired_at END
             WHERE id = $1",
        )
        .bind(id)
        .bind(change.name.as_deref().map(str::trim))
        .bind(change.consumer_client_id.as_deref().map(str::trim))
        .bind(change.retire)
        .execute(&mut *transaction)
        .await?;
        Ok::<_, OperatorError>(Changed::Written(transaction, namespace))
    }
    .await;
    let (transaction, namespace) = match outcome {
        Ok(Changed::Written(transaction, namespace)) => (transaction, namespace),
        Ok(Changed::Missing) => return no_such_domain(),
        Ok(Changed::Retired) => {
            return wire::refuse(StatusCode::CONFLICT, IssueType::Conflict, RETIRED);
        }
        Err(error) => return wire::failed(&error),
    };
    let write = Write {
        created: false,
        namespace: &namespace,
        id,
        reason: Some(&change.reason),
    };
    match finish(operator, &parts, transaction, &write).await {
        Ok(domain) => wire::answer(&domain),
        Err(refused) => *refused,
    }
}

/// What the statement of a change found.
enum Changed {
    /// No domain has the id.
    Missing,
    /// The domain is retired.
    Retired,
    /// The domain of this namespace was changed in this open transaction.
    Written(Transaction<'static, Postgres>, String),
}

/// One domain write, its statement run.
#[derive(Debug, Clone, Copy)]
struct Write<'a> {
    /// Whether the domain was created.
    created: bool,
    /// The domain's namespace.
    namespace: &'a str,
    /// The domain's id.
    id: Uuid,
    /// The reason given.
    reason: Option<&'a str>,
}

/// Writes the steward action and the record of `write` in its open
/// `transaction`, commits it and reads the domain back.
async fn finish(
    operator: &Operator,
    parts: &http::request::Parts,
    mut transaction: Transaction<'static, Postgres>,
    write: &Write<'_>,
) -> Result<PseudonymDomain, Box<Response>> {
    let failed = |error: OperatorError| Box::new(wire::failed(&error));
    let recorded = Recorded {
        kind: if write.created {
            "pseudonym-domain-create"
        } else {
            "pseudonym-domain-update"
        },
        actor: wire::actor(parts),
        pseudonym_domain: Some(write.id),
        reason: write
            .reason
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(str::to_owned),
        request_id: wire::request_id(parts),
        ..Recorded::default()
    };
    record_action(&mut *transaction, &recorded)
        .await
        .map_err(failed)?;
    let call = wire::call(operator, parts);
    let exchange = records::operator::pseudonym_domain_write(
        &call,
        write.created,
        write.namespace,
        Outcome::Success,
    );
    wire::record(operator, parts, vec![exchange])
        .await
        .map_err(|error| Box::new(wire::unaudited(&error)))?;
    transaction
        .commit()
        .await
        .map_err(|error| failed(error.into()))?;
    tracing::info!(
        namespace = write.namespace,
        created = write.created,
        "a pseudonymisation domain is written"
    );
    match read_one(operator, write.id).await {
        Ok(Some(domain)) => Ok(domain),
        Ok(None) => Err(failed(OperatorError::Unadmitted("pseudonymisation domain"))),
        Err(error) => Err(failed(error)),
    }
}

/// Returns the `404` of an id that names no domain.
fn no_such_domain() -> Response {
    wire::refuse(StatusCode::NOT_FOUND, IssueType::NotFound, NO_SUCH_DOMAIN)
}

/// Answers `POST {base}/operator/pseudonyms`: the survivor's pseudonym in
/// the domain, minted when it has none.
pub async fn mint(operator: &Operator, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let (asked, _bytes) = match wire::posted::<Mint>(&parts, body).await {
        Ok(read) => read,
        Err(refused) => return *refused,
    };
    let Some(domain) = wire::uuid(&asked.domain) else {
        return no_such_domain();
    };
    let Some(person) = wire::uuid(&asked.person) else {
        return wire::refuse(StatusCode::NOT_FOUND, IssueType::NotFound, NO_SUCH_PERSON);
    };
    let outcome = async {
        let mut transaction = operator.store.pool().begin().await?;
        let retired: Option<bool> = sqlx::query_scalar(
            "SELECT retired_at IS NOT NULL FROM pix.pseudonym_domain WHERE id = $1",
        )
        .bind(domain)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(retired) = retired else {
            return Ok(Minting::NoDomain);
        };
        if retired {
            return Ok(Minting::Retired);
        }
        let Some(survivor) = survivor(&mut transaction, person).await? else {
            return Ok(Minting::NoPerson);
        };
        let resolved = pseudonym::resolve(&mut transaction, survivor, domain, true).await?;
        let minted = resolved.is_some_and(|resolved| resolved.minted);
        Ok::<_, OperatorError>(Minting::Done(transaction, survivor, minted))
    }
    .await;
    let (mut transaction, survivor, minted) = match outcome {
        Ok(Minting::Done(transaction, survivor, minted)) => (transaction, survivor, minted),
        Ok(Minting::NoDomain) => return no_such_domain(),
        Ok(Minting::Retired) => {
            return wire::refuse(StatusCode::CONFLICT, IssueType::Conflict, RETIRED);
        }
        Ok(Minting::NoPerson) => {
            return wire::refuse(StatusCode::NOT_FOUND, IssueType::NotFound, NO_SUCH_PERSON);
        }
        Err(error) => return wire::failed(&error),
    };
    if minted {
        let recorded = Recorded {
            kind: "pseudonym-mint",
            actor: wire::actor(&parts),
            person: Some(survivor.uuid()),
            pseudonym_domain: Some(domain),
            request_id: wire::request_id(&parts),
            ..Recorded::default()
        };
        if let Err(error) = record_action(&mut *transaction, &recorded).await {
            return wire::failed(&error);
        }
        let namespace: Result<String, sqlx::Error> =
            sqlx::query_scalar("SELECT namespace FROM pix.pseudonym_domain WHERE id = $1")
                .bind(domain)
                .fetch_one(&mut *transaction)
                .await;
        let namespace = match namespace {
            Ok(namespace) => namespace,
            Err(error) => return wire::failed(&error.into()),
        };
        let call = wire::call(operator, &parts);
        let exchange = records::pseudonym_minted(
            &call,
            &format!("Patient/{}", survivor.uuid()),
            &namespace,
            Outcome::Success,
        );
        if let Err(error) = wire::record(operator, &parts, vec![exchange]).await {
            return wire::unaudited(&error);
        }
    }
    if let Err(error) = transaction.commit().await {
        return wire::failed(&error.into());
    }
    wire::answer(&Minted {
        person: survivor.uuid().to_string(),
        domain: domain.to_string(),
        created: minted,
    })
}

/// What a mint found.
enum Minting {
    /// No domain has the id.
    NoDomain,
    /// The domain is retired.
    Retired,
    /// No person has the id.
    NoPerson,
    /// The survivor's pseudonym is held, minted by this request or not.
    Done(Transaction<'static, Postgres>, PersonId, bool),
}

/// Returns the active survivor of the person `person`, `None` when no person
/// has the id.
async fn survivor(
    transaction: &mut Transaction<'static, Postgres>,
    person: Uuid,
) -> Result<Option<PersonId>, OperatorError> {
    let found = crate::pdqm::patient::persons(&mut *transaction, &[person]).await?;
    Ok(found
        .get(&person)
        .map(|person| PersonId::from_uuid(person.survivor.unwrap_or(person.id))))
}

/// Answers `POST {base}/operator/pseudonyms/resolve`: the person a pseudonym
/// resolves to, disclosed with the steward's reason.
pub async fn resolve(operator: &Operator, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let (asked, _bytes) = match wire::posted::<Resolve>(&parts, body).await {
        Ok(read) => read,
        Err(refused) => return *refused,
    };
    if !reasoned(&asked.reason) {
        return wire::refuse(StatusCode::BAD_REQUEST, IssueType::Invalid, NO_REASON);
    }
    let Some(value) = wire::uuid(asked.pseudonym.trim()) else {
        return not_resolved();
    };
    let outcome = async {
        let mut transaction = operator.store.pool().begin().await?;
        let found = pseudonym::reverse(&mut transaction, &asked.namespace, value).await?;
        let domain: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM pix.pseudonym_domain WHERE namespace = $1")
                .bind(&asked.namespace)
                .fetch_optional(&mut *transaction)
                .await?;
        Ok::<_, OperatorError>((transaction, found, domain))
    }
    .await;
    let (mut transaction, person, domain) = match outcome {
        Ok((transaction, Some(person), domain)) => (transaction, person, domain),
        Ok((_, None, _)) => return not_resolved(),
        Err(error) => return wire::failed(&error),
    };
    let reason = asked.reason.trim();
    let recorded = Recorded {
        kind: "pseudonym-resolve",
        actor: wire::actor(&parts),
        person: Some(person.uuid()),
        pseudonym_domain: domain,
        reason: Some(reason.to_owned()),
        request_id: wire::request_id(&parts),
        ..Recorded::default()
    };
    let action = match record_action(&mut *transaction, &recorded).await {
        Ok(action) => action,
        Err(error) => return wire::failed(&error),
    };
    let call = wire::call(operator, &parts);
    let exchange = records::operator::disclosure(
        &call,
        &format!("Patient/{}", person.uuid()),
        Disclosed {
            kind: "pseudonym-resolution",
            system: &asked.namespace,
            reason,
        },
        Outcome::Success,
    );
    if let Err(error) = wire::record(operator, &parts, vec![exchange]).await {
        return wire::unaudited(&error);
    }
    if let Err(error) = transaction.commit().await {
        return wire::failed(&error.into());
    }
    wire::answer(&Resolution {
        person: person.uuid().to_string(),
        action: action.to_string(),
    })
}

/// Returns the `404` of a pseudonym that resolves to no person.
fn not_resolved() -> Response {
    wire::refuse(StatusCode::NOT_FOUND, IssueType::NotFound, NOT_RESOLVED)
}
