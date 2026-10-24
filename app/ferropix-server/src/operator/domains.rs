// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The identifier domains: `GET {base}/operator/domains`, a page of them by
//! system after a cursor, and `GET {base}/operator/domains/{id}`, one with
//! its last feeds.
//!
//! A domain is listed with its record count and its last feed, retired ones
//! included; a feed row names its time, its channel and whether it created
//! or revised the record, never a value. Neither reads a person, so neither
//! writes an audit record. Our own design (`docs/architecture.md` §12.3).

use axum::extract::Request;
use axum::response::Response;
use ferropix_operator_api::domain::{
    DEFAULT_LIMIT, Domain, DomainPage, DomainQuery, Domains, FEEDS, Feed, FeedOutcome, MAX_LIMIT,
};
use http::StatusCode;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::fhir::outcome::IssueType;
use crate::operator::{Operator, OperatorError, channel, count, domain_kind, snapshot, wire};

/// The diagnostics of an id that names no domain.
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
        Ok(page) => wire::answer(&page),
        Err(error) => wire::failed(&error),
    }
}

/// Answers one domain with its last feeds.
pub async fn one(operator: &Operator, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let Some(id) = wire::path_id(&parts) else {
        return not_found();
    };
    match read_one(operator, id).await {
        Ok(Some(page)) => wire::answer(&page),
        Ok(None) => not_found(),
        Err(error) => wire::failed(&error),
    }
}

/// Returns the `404` of an id that names no domain.
fn not_found() -> Response {
    wire::refuse(StatusCode::NOT_FOUND, IssueType::NotFound, NO_SUCH_DOMAIN)
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
