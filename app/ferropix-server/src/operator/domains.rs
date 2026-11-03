// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The identifier domains: `GET {base}/operator/domains`, a page of them by
//! system after a cursor, and `GET {base}/operator/domains/{id}`, one with
//! its last feeds.
//!
//! A domain is listed with its record count and its last feed, retired ones
//! included; a feed row names its time, its channel and whether it created
//! or revised the record, never a value. Neither read names a person, so
//! neither writes an audit record.
//!
//! Behind the administrator scope, `POST {base}/operator/domains` adds a
//! local or authoritative domain and `PATCH {base}/operator/domains/{id}`
//! renames one, gives it another Source or retires it. A Source must be a
//! client an issuer lists; a retired domain stays retired. Each write is one
//! transaction with its `domain-create` or `domain-update` steward action and
//! its BALP record, and the guard takes the store's Sources the moment it
//! commits (PIXm 3.1.0 §2:3.104.5). Our own design (`docs/architecture.md`
//! §12.3).

use crate::fhir::outcome;
use axum::extract::Request;
use axum::response::Response;
use ferropix_operator_api::domain::{
    DEFAULT_LIMIT, Domain, DomainChange, DomainPage, DomainQuery, Domains, FEEDS, Feed,
    FeedOutcome, MAX_LIMIT, NewDomain,
};
use ferropix_operator_api::patient::DomainKind;
use http::StatusCode;
use ihe_atna::balp::Outcome;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::audit::records;
use crate::config::section::domain::is_system;
use crate::operator::{
    Operator, OperatorError, change, channel, count, domain_kind, snapshot, wire,
};
use crate::problem::{self, Kind};
use crate::store::seed;

/// The detail of an id that names no domain.
pub const NO_SUCH_DOMAIN: &str = "no identifier domain has this id";

/// The columns of one domain row, each timestamp RFC 3339 UTC.
const COLUMNS: &str = "d.id, d.system, d.name, d.kind, d.sensitive, d.source_client_id,
     to_char(d.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS created_at,
     to_char(d.retired_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS retired_at,
     (SELECT count(*) FROM pix.source_record r WHERE r.domain_id = d.id) AS records,
     (SELECT to_char(max(v.received_at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
      FROM pix.source_record_version v JOIN pix.source_record r ON r.id = v.record_id
      WHERE r.domain_id = d.id) AS last_fed";

/// One domain as the store holds it.
#[derive(sqlx::FromRow)]
struct DomainRow {
    id: Uuid,
    system: String,
    name: String,
    kind: String,
    sensitive: bool,
    source_client_id: String,
    created_at: String,
    retired_at: Option<String>,
    records: i64,
    last_fed: Option<String>,
}

impl DomainRow {
    /// Returns the document's domain.
    fn document(self) -> Result<Domain, OperatorError> {
        Ok(Domain {
            id: self.id.to_string(),
            system: self.system,
            name: self.name,
            kind: domain_kind(&self.kind)?,
            sensitive: self.sensitive,
            source_client_id: self.source_client_id,
            created_at: self.created_at,
            retired_at: self.retired_at,
            records: count(self.records),
            last_fed: self.last_fed,
        })
    }
}

/// Answers a page of domains.
pub async fn list(operator: &Operator, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let query: DomainQuery = match wire::query(&parts) {
        Ok(query) => query,
        Err(refused) => return *refused,
    };
    match read_page(operator, &query).await {
        Ok(page) => outcome::body(StatusCode::OK, &page, wire::JSON),
        Err(error) => wire::failed(&error),
    }
}

/// Answers one domain with its last feeds.
pub async fn one(operator: &Operator, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let Some(id) = wire::path_id(&parts) else {
        return problem::refuse(Kind::NotFound, NO_SUCH_DOMAIN);
    };
    match read_one(operator, id).await {
        Ok(Some(page)) => outcome::body(StatusCode::OK, &page, wire::JSON),
        Ok(None) => problem::refuse(Kind::NotFound, NO_SUCH_DOMAIN),
        Err(error) => wire::failed(&error),
    }
}

/// Reads the page of domains `query` asks for.
///
/// # Errors
///
/// Returns [`OperatorError`] when the store cannot be read.
pub async fn read_page(operator: &Operator, query: &DomainQuery) -> Result<Domains, OperatorError> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let rows: Vec<DomainRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM pix.identifier_domain d
         WHERE $1::text IS NULL OR d.system > $1
         ORDER BY d.system
         LIMIT $2"
    )))
    .bind(query.after.as_deref())
    .bind(i64::from(limit).saturating_add(1))
    .fetch_all(operator.store.pool())
    .await?;
    let size = usize::try_from(limit).unwrap_or(usize::MAX);
    let more = rows.len() > size;
    let domains = rows
        .into_iter()
        .take(size)
        .map(DomainRow::document)
        .collect::<Result<Vec<_>, _>>()?;
    let next = more
        .then(|| domains.last().map(|domain| domain.system.clone()))
        .flatten();
    Ok(Domains { domains, next })
}

/// Reads the domain `id` with its last feeds, or `None` when the store
/// holds none.
///
/// # Errors
///
/// Returns [`OperatorError`] when the store cannot be read.
pub async fn read_one(operator: &Operator, id: Uuid) -> Result<Option<DomainPage>, OperatorError> {
    let mut snapshot = snapshot(&operator.store).await?;
    let row: Option<DomainRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM pix.identifier_domain d WHERE d.id = $1"
    )))
    .bind(id)
    .fetch_optional(&mut *snapshot)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let feeds = feeds(&mut snapshot, id).await?;
    snapshot.commit().await?;
    Ok(Some(DomainPage {
        domain: row.document()?,
        feeds,
    }))
}

/// Reads the last [`FEEDS`] versions the domain `id` received.
async fn feeds(connection: &mut PgConnection, id: Uuid) -> Result<Vec<Feed>, OperatorError> {
    let limit = i64::try_from(FEEDS).map_err(|_overflow| OperatorError::Unadmitted("limit"))?;
    let rows: Vec<(String, String, i32)> = sqlx::query_as(
        "SELECT to_char(v.received_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
                v.channel, v.version
         FROM pix.source_record_version v
         JOIN pix.source_record r ON r.id = v.record_id
         WHERE r.domain_id = $1
         ORDER BY v.received_at DESC, v.id DESC
         LIMIT $2",
    )
    .bind(id)
    .bind(limit)
    .fetch_all(&mut *connection)
    .await?;
    rows.into_iter()
        .map(|(at, stored, version)| {
            Ok(Feed {
                at,
                channel: channel(&stored)?,
                outcome: if version == 1 {
                    FeedOutcome::Created
                } else {
                    FeedOutcome::Revised
                },
            })
        })
        .collect()
}

/// The detail of a domain write whose fields do not hold.
pub const NOT_A_DOMAIN: &str = "a domain names an absolute URI as its system, a name and a Source, and is local or authoritative";

/// The detail of a Source no issuer lists.
pub const SOURCE_NOT_ADMITTED: &str =
    "no [[auth.issuer]] lists this client among its clients, and no Basic user is it";

/// The detail of a system another domain holds.
pub const SYSTEM_TAKEN: &str = "another identifier domain holds this system";

/// The detail of a change to a retired domain.
pub const RETIRED: &str = "a retired identifier domain stays as it was retired";

/// The detail of a change without a reason.
pub const NO_REASON: &str = "a change of an identifier domain states its reason";

/// Answers `POST {base}/operator/domains`: a new domain, its Source admitted
/// at once.
pub async fn create(operator: &Operator, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let (new, _bytes) = match wire::posted::<NewDomain>(&parts, body).await {
        Ok(read) => read,
        Err(refused) => return *refused,
    };
    let kind = match new.kind {
        DomainKind::Local => "local",
        DomainKind::Authoritative => "authoritative",
        DomainKind::Master => {
            return problem::refuse(Kind::InvalidRequest, NOT_A_DOMAIN);
        }
    };
    if !is_system(&new.system)
        || new.name.trim().is_empty()
        || new.source_client_id.trim().is_empty()
    {
        return problem::refuse(Kind::InvalidRequest, NOT_A_DOMAIN);
    }
    if !operator.sources.admits(new.source_client_id.trim()) {
        return problem::refuse(Kind::SourceNotAdmitted, SOURCE_NOT_ADMITTED);
    }
    let written = change::insert(
        operator,
        &IDENTIFIER_DOMAINS,
        sqlx::query_scalar(
            "INSERT INTO pix.identifier_domain (system, name, kind, source_client_id, sensitive)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (system) DO NOTHING
             RETURNING id",
        )
        .bind(&new.system)
        .bind(new.name.trim())
        .bind(kind)
        .bind(new.source_client_id.trim())
        .bind(new.sensitive.unwrap_or(true)),
        (&new.system, new.reason.as_deref()),
    )
    .await;
    match finish(operator, &parts, written).await {
        Ok(page) => outcome::body(StatusCode::CREATED, &page.domain, wire::JSON),
        Err(refused) => *refused,
    }
}

/// Answers `PATCH {base}/operator/domains/{id}`: a new name, a new Source or
/// the domain retired.
pub async fn update(operator: &Operator, request: Request) -> Response {
    let (parts, id, change) = match change::requested::<DomainChange>(request, NO_SUCH_DOMAIN).await
    {
        Ok(requested) => requested,
        Err(refused) => return *refused,
    };
    if change.reason.trim().is_empty() {
        return problem::refuse(Kind::ReasonRequired, NO_REASON);
    }
    if change::blank(change.name.as_deref()) || change::blank(change.source_client_id.as_deref()) {
        return problem::refuse(Kind::InvalidRequest, NOT_A_DOMAIN);
    }
    if let Some(source) = &change.source_client_id
        && !operator.sources.admits(source.trim())
    {
        return problem::refuse(Kind::SourceNotAdmitted, SOURCE_NOT_ADMITTED);
    }
    let asked = change::Asked {
        name: change.name.as_deref(),
        holder: change.source_client_id.as_deref(),
        retire: change.retire,
        reason: &change.reason,
    };
    let written = change::change(operator, &IDENTIFIER_DOMAINS, (id, asked)).await;
    match finish(operator, &parts, written).await {
        Ok(page) => outcome::body(StatusCode::OK, &page.domain, wire::JSON),
        Err(refused) => *refused,
    }
}

/// The statements and the refusals of a write of an identifier domain.
const IDENTIFIER_DOMAINS: change::Domains = change::Domains {
    lock: "SELECT system, retired_at IS NOT NULL FROM pix.identifier_domain
             WHERE id = $1 FOR UPDATE",
    update: "UPDATE pix.identifier_domain
             SET name = coalesce($2, name),
                 source_client_id = coalesce($3, source_client_id),
                 retired_at = CASE WHEN $4 THEN now() ELSE retired_at END
             WHERE id = $1",
    taken: SYSTEM_TAKEN,
    missing: NO_SUCH_DOMAIN,
    retired: RETIRED,
    actions: ("domain-create", "domain-update"),
};

/// Writes the steward action and the BALP record of `write` in its open
/// `transaction` and commits it; then gives the guard the store's Sources
/// and reads the domain back.
async fn finish(
    operator: &Operator,
    parts: &http::request::Parts,
    written: change::Written<'_>,
) -> Result<DomainPage, Box<Response>> {
    let failed = |error: OperatorError| Box::new(wire::failed(&error));
    let write = change::commit(
        (operator, parts),
        &IDENTIFIER_DOMAINS,
        written,
        |recorded, write| recorded.domain = Some(write.id),
        |call, write| {
            records::operator::domain_write(call, write.created, &write.key, Outcome::Success)
        },
    )
    .await?;
    let stored = seed::stored(&operator.store)
        .await
        .map_err(|error| failed(error.into()))?;
    operator.sources.replace(&stored);
    tracing::info!(
        system = &write.key,
        created = write.created,
        "an identifier domain is written"
    );
    match read_one(operator, write.id).await {
        Ok(Some(page)) => Ok(page),
        Ok(None) => Err(failed(OperatorError::Unadmitted("identifier domain"))),
        Err(error) => Err(failed(error)),
    }
}
