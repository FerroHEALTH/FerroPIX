// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The audit trail: `GET {base}/operator/actions`, a page of steward and
//! system actions the newest first, and `POST {base}/operator/actions/export`,
//! the same filter answered as CSV.
//!
//! The filter narrows by actor, kind, person and time; a page continues
//! after the id of the last action of the page before, and holds at most
//! [`MAX_LIMIT`] actions, an export at most [`EXPORT_LIMIT`] with the cursor
//! of the rest in [`EXPORT_NEXT`]. An export is itself a steward action and
//! writes a BALP Export record before it answers, so the trail records who
//! took it away. A cell that a spreadsheet would read as a formula is
//! prefixed with `'`. These rules are our own design.

use axum::body::Body;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use ferropix_operator_api::actions::{
    Action, ActionFilter, Actions, DEFAULT_LIMIT, EXPORT_LIMIT, EXPORT_NEXT, MAX_LIMIT,
};
use http::{HeaderValue, StatusCode, header};
use ihe_atna::balp::Outcome;
use uuid::Uuid;

use crate::audit::records;
use crate::fhir::outcome::IssueType;
use crate::operator::{Operator, OperatorError, Recorded, record_action, wire};

/// The media type of an export (RFC 4180 §3).
pub const CSV: &str = "text/csv; charset=utf-8";

/// The header row of an export.
const HEADER: &str =
    "id,kind,actor,at,person,record,candidate,merge,domain,pseudonym_domain,reason,request_id\r\n";

/// A filter whose values have been read.
#[derive(Debug, Clone, Default)]
struct Filter {
    actor: Option<String>,
    kind: Option<String>,
    person: Option<Uuid>,
    from: Option<String>,
    to: Option<String>,
    after: Option<Uuid>,
}

impl Filter {
    /// Reads `filter`, or `None` when a person, a cursor or an instant does
    /// not read.
    fn of(filter: &ActionFilter) -> Option<Self> {
        let instant = |text: &Option<String>| -> Option<Option<String>> {
            match text {
                None => Some(None),
                Some(text) => text
                    .parse::<jiff::Timestamp>()
                    .ok()
                    .map(|instant| Some(instant.to_string())),
            }
        };
        let id = |text: &Option<String>| -> Option<Option<Uuid>> {
            match text {
                None => Some(None),
                Some(text) => wire::uuid(text).map(Some),
            }
        };
        Some(Self {
            actor: filter.actor.clone(),
            kind: filter.kind.clone(),
            person: id(&filter.person)?,
            from: instant(&filter.from)?,
            to: instant(&filter.to)?,
            after: id(&filter.after)?,
        })
    }
}

/// One action as the store holds it.
#[derive(sqlx::FromRow)]
struct ActionRow {
    id: Uuid,
    kind: String,
    actor: String,
    at: String,
    person_id: Option<Uuid>,
    record_id: Option<Uuid>,
    candidate_id: Option<Uuid>,
    merge_id: Option<Uuid>,
    domain_id: Option<Uuid>,
    pseudonym_domain_id: Option<Uuid>,
    reason: Option<String>,
    request_id: Option<String>,
}

impl From<ActionRow> for Action {
    fn from(row: ActionRow) -> Self {
        Self {
            id: row.id.to_string(),
            kind: row.kind,
            actor: row.actor,
            at: row.at,
            person: row.person_id.map(|id| id.to_string()),
            record: row.record_id.map(|id| id.to_string()),
            candidate: row.candidate_id.map(|id| id.to_string()),
            merge: row.merge_id.map(|id| id.to_string()),
            domain: row.domain_id.map(|id| id.to_string()),
            pseudonym_domain: row.pseudonym_domain_id.map(|id| id.to_string()),
            reason: row.reason,
            request_id: row.request_id,
        }
    }
}

/// Answers a page of the trail.
pub async fn list(operator: &Operator, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let filter: ActionFilter = match wire::query(&parts) {
        Ok(filter) => filter,
        Err(refused) => return *refused,
    };
    let Some(read) = Filter::of(&filter) else {
        return unread_filter();
    };
    let limit = filter.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    match page(operator, &read, limit).await {
        Ok((actions, more)) => {
            let next = more
                .then(|| actions.last().map(|action| action.id.clone()))
                .flatten();
            wire::answer(&Actions { actions, next })
        }
        Err(error) => wire::failed(&error),
    }
}

/// Answers an export of the trail and records it.
pub async fn export(operator: &Operator, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let (filter, _bytes) = match wire::posted::<ActionFilter>(&parts, body).await {
        Ok(read) => read,
        Err(refused) => return *refused,
    };
    let Some(read) = Filter::of(&filter) else {
        return unread_filter();
    };
    let limit = filter.limit.unwrap_or(EXPORT_LIMIT).clamp(1, EXPORT_LIMIT);
    let (actions, more) = match page(operator, &read, limit).await {
        Ok(page) => page,
        Err(error) => return wire::failed(&error),
    };
    let recorded = Recorded {
        kind: "export",
        actor: wire::actor(&parts),
        person: read.person,
        request_id: wire::request_id(&parts),
        ..Recorded::default()
    };
    if let Err(error) = record_action(operator.store.pool(), &recorded).await {
        return wire::failed(&error);
    }
    let call = wire::call(operator, &parts);
    let person = read.person.map(|person| format!("Patient/{person}"));
    let exchange =
        records::operator::export(&call, actions.len(), person.as_deref(), Outcome::Success);
    if let Err(error) = wire::record(operator, &parts, vec![exchange]).await {
        return wire::unaudited(&error);
    }
    let next = more
        .then(|| actions.last().map(|action| action.id.clone()))
        .flatten();
    let mut response = (StatusCode::OK, Body::from(csv(&actions))).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(CSV));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"ferropix-actions.csv\""),
    );
    if let Some(value) = next.and_then(|next| HeaderValue::from_str(&next).ok()) {
        headers.insert(EXPORT_NEXT, value);
    }
    response
}

/// Returns the `400` of a filter whose person, cursor or instant does not
/// read.
fn unread_filter() -> Response {
    wire::refuse(
        StatusCode::BAD_REQUEST,
        IssueType::Invalid,
        wire::NOT_THE_QUERY,
    )
}

/// Reads at most `limit` actions `filter` admits, the newest first, and
/// whether more follow.
async fn page(
    operator: &Operator,
    filter: &Filter,
    limit: u32,
) -> Result<(Vec<Action>, bool), OperatorError> {
    let rows: Vec<ActionRow> = sqlx::query_as(
        "SELECT id, kind, actor,
                to_char(at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS at,
                person_id, record_id, candidate_id, merge_id, domain_id,
                pseudonym_domain_id, reason, request_id
         FROM pix.steward_action
         WHERE ($1::text IS NULL OR actor = $1)
           AND ($2::text IS NULL OR kind = $2)
           AND ($3::uuid IS NULL OR person_id = $3)
           AND ($4::text IS NULL OR at >= $4::timestamptz)
           AND ($5::text IS NULL OR at < $5::timestamptz)
           AND ($6::uuid IS NULL OR id < $6)
         ORDER BY id DESC
         LIMIT $7",
    )
    .bind(filter.actor.as_deref())
    .bind(filter.kind.as_deref())
    .bind(filter.person)
    .bind(filter.from.as_deref())
    .bind(filter.to.as_deref())
    .bind(filter.after)
    .bind(i64::from(limit).saturating_add(1))
    .fetch_all(operator.store.pool())
    .await?;
    let size = usize::try_from(limit).unwrap_or(usize::MAX);
    let more = rows.len() > size;
    Ok((
        rows.into_iter().take(size).map(Action::from).collect(),
        more,
    ))
}

/// Returns `actions` as CSV with its header row (RFC 4180 §2).
#[must_use]
pub fn csv(actions: &[Action]) -> String {
    let mut text = String::from(HEADER);
    for action in actions {
        let cells = [
            Some(action.id.as_str()),
            Some(action.kind.as_str()),
            Some(action.actor.as_str()),
            Some(action.at.as_str()),
            action.person.as_deref(),
            action.record.as_deref(),
            action.candidate.as_deref(),
            action.merge.as_deref(),
            action.domain.as_deref(),
            action.pseudonym_domain.as_deref(),
            action.reason.as_deref(),
            action.request_id.as_deref(),
        ];
        let line: Vec<String> = cells
            .iter()
            .map(|cell| cell.map(field).unwrap_or_default())
            .collect();
        text.push_str(&line.join(","));
        text.push_str("\r\n");
    }
    text
}

/// Returns `value` as one CSV field: quoted when it holds a separator, a
/// quote or a line break, and prefixed with `'` when a spreadsheet would
/// read it as a formula.
fn field(value: &str) -> String {
    let guarded = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

#[cfg(test)]
mod tests {
    use ferropix_operator_api::actions::{Action, ActionFilter};

    use super::{Filter, csv};

    #[test]
    fn a_reason_with_a_separator_is_quoted_and_a_formula_is_defused() {
        let action = Action {
            id: String::from("a-1"),
            kind: String::from("view"),
            actor: String::from("=steward"),
            at: String::from("2026-10-24T10:00:00.000000Z"),
            person: Some(String::from("p-1")),
            record: None,
            candidate: None,
            merge: None,
            domain: None,
            pseudonym_domain: None,
            reason: Some(String::from("asked, \"twice\"\nby phone")),
            request_id: None,
        };
        let text = csv(&[action]);
        let mut lines = text.split("\r\n");
        assert_eq!(
            Some(
                "id,kind,actor,at,person,record,candidate,merge,domain,pseudonym_domain,reason,request_id"
            ),
            lines.next()
        );
        assert_eq!(
            "a-1,view,'=steward,2026-10-24T10:00:00.000000Z,p-1,,,,,,\"asked, \"\"twice\"\"\nby phone\",",
            text.split_once("\r\n")
                .map(|(_, rest)| rest.trim_end())
                .unwrap_or_default()
        );
    }

    #[test]
    fn a_filter_whose_person_cursor_or_instant_does_not_read_is_refused() {
        assert!(Filter::of(&ActionFilter::default()).is_some());
        for filter in [
            ActionFilter {
                person: Some(String::from("not-a-uuid")),
                ..ActionFilter::default()
            },
            ActionFilter {
                after: Some(String::from("x")),
                ..ActionFilter::default()
            },
            ActionFilter {
                from: Some(String::from("yesterday")),
                ..ActionFilter::default()
            },
        ] {
            assert!(Filter::of(&filter).is_none(), "{filter:?}");
        }
        let read = Filter::of(&ActionFilter {
            from: Some(String::from("2026-10-24T10:00:00+02:00")),
            ..ActionFilter::default()
        })
        .expect("an RFC 3339 instant reads");
        assert_eq!(Some("2026-10-24T08:00:00Z"), read.from.as_deref());
    }
}
