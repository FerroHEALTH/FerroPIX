// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! `GET {base}/operator/persons/{id}`: the person with its identifiers per
//! domain, its source records, its history and its pseudonyms, read in one
//! snapshot.
//!
//! A record belongs to the person while its latest link names the person:
//! open while the record is active, closed when the Source subsumed or
//! removed it. Every identifier is opened to be masked and only its mask
//! leaves the server. Each answer writes a `view` steward action and the
//! BALP Read record of the person (BALP 1.1.4 §3:5.7.3) before it is sent;
//! an unknown person writes neither. What the document holds is our own
//! design (`docs/architecture.md` §12.3).

use axum::extract::Request;
use axum::response::Response;
use ferropix_operator_api::patient::{DomainRef, Masked, PersonStatus};
use ferropix_operator_api::person::{
    HISTORY, HistoryEntry, HistoryEvent, IdentifierHandle, IdentifierRow, Link, LinkKind, Person,
    PseudonymRow, RecordState, SourceRecord,
};
use http::StatusCode;
use ihe_atna::balp::Outcome;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::audit::records::{self, StewardAction};
use crate::fhir::outcome::IssueType;
use crate::matching::scorer::mask;
use crate::operator::{
    Operator, OperatorError, Recorded, channel, demographics, domain_kind, opened, record_action,
    snapshot, wire,
};
use crate::pdqm::patient::{SERVED, SERVED_ORDER};
use crate::store::seal::Sealer;

/// The format every timestamp of the document is written in, RFC 3339 UTC.
const UTC: &str = "YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"";

/// The pseudonymisation domains a document lists at most.
const PSEUDONYM_DOMAINS: i64 = 200;

/// Answers the person the matched route's `{id}` names and records the view.
pub async fn answer(operator: &Operator, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let Some(id) = wire::path_id(&parts) else {
        return not_found();
    };
    let person = match read(operator, id).await {
        Ok(Some(person)) => person,
        Ok(None) => return not_found(),
        Err(error) => return wire::failed(&error),
    };
    let recorded = Recorded {
        kind: "view",
        actor: wire::actor(&parts),
        person: Some(id),
        request_id: wire::request_id(&parts),
        ..Recorded::default()
    };
    if let Err(error) = record_action(operator.store.pool(), &recorded).await {
        return wire::failed(&error);
    }
    let call = wire::call(operator, &parts);
    let exchange = records::steward(
        &call,
        StewardAction::View,
        &format!("Patient/{id}"),
        Outcome::Success,
    );
    if let Err(error) = wire::record(operator, &parts, vec![exchange]).await {
        return wire::unaudited(&error);
    }
    wire::answer(&person)
}

/// Returns the `404` of an id that names no person.
fn not_found() -> Response {
    wire::refuse(
        StatusCode::NOT_FOUND,
        IssueType::NotFound,
        wire::NO_SUCH_PERSON,
    )
}

/// Reads the person `id`, or `None` when the store holds none.
///
/// # Errors
///
/// Returns [`OperatorError`] when the store cannot be read, a sealed value
/// does not open or stored demographics are no `Patient`.
pub async fn read(operator: &Operator, id: Uuid) -> Result<Option<Person>, OperatorError> {
    let mut snapshot = snapshot(&operator.store).await?;
    let row: Option<(String, Option<Uuid>, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT status, merged_into, to_char(created_at AT TIME ZONE 'UTC', '{UTC}')
         FROM pix.person WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(&mut *snapshot)
    .await?;
    let Some((status, merged_into, created_at)) = row else {
        return Ok(None);
    };
    let status = match status.as_str() {
        "active" => PersonStatus::Active,
        "merged" => PersonStatus::Merged,
        _ => return Err(OperatorError::Unadmitted("person status")),
    };
    let leading = leading(&mut snapshot, id).await?;
    let sealer = operator.store.sealer();
    let (mut identifiers, records) = records(&mut snapshot, sealer, id).await?;
    identifiers.extend(carried(&mut snapshot, sealer, id).await?);
    identifiers.sort_by(|left, right| left.domain.system.cmp(&right.domain.system));
    let (history, history_truncated) = history(&mut snapshot, id).await?;
    let pseudonyms = pseudonyms(&mut snapshot, id).await?;
    snapshot.commit().await?;
    Ok(Some(Person {
        id: id.to_string(),
        status,
        merged_into: merged_into.map(|survivor| survivor.to_string()),
        created_at,
        leading,
        identifiers,
        records,
        history,
        history_truncated,
        pseudonyms,
    }))
}

/// Reads the demographics the PDQm Supplier serves for `person`.
pub(super) async fn leading(
    connection: &mut PgConnection,
    person: Uuid,
) -> Result<Option<ferropix_operator_api::patient::Demographics>, OperatorError> {
    let served: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT demographics::text FROM ({SERVED} AND l.person_id = $1 {SERVED_ORDER}) s"
    )))
    .bind(person)
    .fetch_optional(&mut *connection)
    .await?;
    Ok(served.as_deref().map(demographics::stored).transpose()?)
}

/// One record of the person as the store holds it.
#[derive(sqlx::FromRow)]
struct RecordRow {
    id: Uuid,
    domain_id: Uuid,
    system: String,
    name: String,
    kind: String,
    sealed: Option<Vec<u8>>,
    plain: Option<String>,
    subsumed: bool,
    removed: bool,
    version: i32,
    received_at: String,
    channel: String,
    demographics: String,
    link_kind: String,
    made_by: String,
    made_at: String,
    open: bool,
}

/// Reads the records whose latest link names `person`, with their
/// identifiers masked.
async fn records(
    connection: &mut PgConnection,
    sealer: &Sealer,
    person: Uuid,
) -> Result<(Vec<IdentifierRow>, Vec<SourceRecord>), OperatorError> {
    let rows: Vec<RecordRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "WITH latest AS (
             SELECT DISTINCT ON (l.record_id)
                    l.record_id, l.person_id, l.kind, l.made_by, l.made_at,
                    upper_inf(l.valid) AS open
             FROM pix.link l
             WHERE l.record_id IN (SELECT record_id FROM pix.link WHERE person_id = $1)
             ORDER BY l.record_id, lower(l.valid) DESC, l.made_at DESC
         )
         SELECT r.id, d.id AS domain_id, d.system, d.name, d.kind,
                r.identifier_sealed AS sealed, r.identifier_value AS plain,
                r.replaced_by IS NOT NULL AS subsumed, r.removed_at IS NOT NULL AS removed,
                v.version, to_char(v.received_at AT TIME ZONE 'UTC', '{UTC}') AS received_at,
                v.channel, v.demographics::text AS demographics,
                la.kind AS link_kind, la.made_by,
                to_char(la.made_at AT TIME ZONE 'UTC', '{UTC}') AS made_at, la.open
         FROM latest la
         JOIN pix.source_record r ON r.id = la.record_id
         JOIN pix.identifier_domain d ON d.id = r.domain_id
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
         WHERE la.person_id = $1 AND d.retired_at IS NULL
           AND (la.open OR r.replaced_by IS NOT NULL OR r.removed_at IS NOT NULL)
         ORDER BY d.system, r.id"
    )))
    .bind(person)
    .fetch_all(&mut *connection)
    .await?;
    let mut identifiers = Vec::with_capacity(rows.len());
    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        let domain = DomainRef {
            id: row.domain_id.to_string(),
            system: row.system,
            name: row.name,
            kind: domain_kind(&row.kind)?,
        };
        let value = opened(
            sealer,
            row.domain_id,
            row.sealed.as_deref(),
            row.plain.as_deref(),
        )?;
        let state = if row.removed {
            RecordState::Removed
        } else if row.subsumed {
            RecordState::Subsumed
        } else {
            RecordState::Active
        };
        identifiers.push(IdentifierRow {
            handle: IdentifierHandle::Record(row.id.to_string()),
            domain: domain.clone(),
            masked: Masked(mask(value.expose())),
            state,
            last_fed: Some(row.received_at.clone()),
            link: if row.open {
                Some(Link {
                    kind: link_kind(&row.link_kind)?,
                    made_by: row.made_by.clone(),
                    made_at: row.made_at.clone(),
                })
            } else {
                None
            },
        });
        records.push(SourceRecord {
            id: row.id.to_string(),
            domain,
            version: u32::try_from(row.version)
                .map_err(|_negative| OperatorError::Unadmitted("version"))?,
            received_at: row.received_at,
            channel: channel(&row.channel)?,
            state,
            demographics: demographics::stored(&row.demographics)?,
        });
    }
    Ok((identifiers, records))
}

/// One carried identifier as the store holds it.
#[derive(sqlx::FromRow)]
struct CarriedRow {
    id: Uuid,
    domain_id: Uuid,
    system: String,
    name: String,
    kind: String,
    sealed: Option<Vec<u8>>,
    plain: Option<String>,
    received_at: String,
}

/// Reads the authoritative identifiers the person's current records carry
/// with no record of their own, masked.
async fn carried(
    connection: &mut PgConnection,
    sealer: &Sealer,
    person: Uuid,
) -> Result<Vec<IdentifierRow>, OperatorError> {
    let rows: Vec<CarriedRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT DISTINCT ON (d.system, c.identifier_hmac)
                c.id, d.id AS domain_id, d.system, d.name, d.kind,
                c.identifier_sealed AS sealed, c.identifier_value AS plain,
                to_char(v.received_at AT TIME ZONE 'UTC', '{UTC}') AS received_at
         FROM pix.link l
         JOIN pix.source_record r ON r.id = l.record_id
         JOIN pix.carried_identifier c ON c.record_id = r.id AND c.version = r.head_version
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
         JOIN pix.identifier_domain d ON d.id = c.domain_id
         WHERE l.person_id = $1 AND upper_inf(l.valid)
           AND r.active AND r.removed_at IS NULL
           AND d.kind = 'authoritative' AND d.retired_at IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM pix.source_record o
               WHERE o.domain_id = c.domain_id AND o.identifier_hmac = c.identifier_hmac
           )
         ORDER BY d.system, c.identifier_hmac, v.received_at DESC"
    )))
    .bind(person)
    .fetch_all(&mut *connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            let value = opened(
                sealer,
                row.domain_id,
                row.sealed.as_deref(),
                row.plain.as_deref(),
            )?;
            Ok(IdentifierRow {
                handle: IdentifierHandle::Carried(row.id.to_string()),
                domain: DomainRef {
                    id: row.domain_id.to_string(),
                    system: row.system,
                    name: row.name,
                    kind: domain_kind(&row.kind)?,
                },
                masked: Masked(mask(value.expose())),
                state: RecordState::Active,
                last_fed: Some(row.received_at),
                link: None,
            })
        })
        .collect()
}

/// One history entry as the store holds it.
#[derive(sqlx::FromRow)]
struct HistoryRow {
    at: String,
    actor: Option<String>,
    reason: Option<String>,
    event: String,
    subject: Option<Uuid>,
    version: Option<i32>,
    detail: Option<String>,
    survivor: Option<Uuid>,
    merged: Option<Uuid>,
}

/// Reads the person's history, the newest first, at most [`HISTORY`]
/// entries, and whether older ones were left out.
async fn history(
    connection: &mut PgConnection,
    person: Uuid,
) -> Result<(Vec<HistoryEntry>, bool), OperatorError> {
    let limit = i64::try_from(HISTORY)
        .map_err(|_overflow| OperatorError::Unadmitted("limit"))?
        .saturating_add(1);
    let rows: Vec<HistoryRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT to_char(h.at AT TIME ZONE 'UTC', '{UTC}') AS at, h.actor, h.reason, h.event,
                h.subject, h.version, h.detail, h.survivor, h.merged
         FROM (
             SELECT v.received_at AS at, NULL::text AS actor, NULL::text AS reason,
                    'version' AS event, v.record_id AS subject, v.version, v.channel AS detail,
                    NULL::uuid AS survivor, NULL::uuid AS merged, v.id AS tie
             FROM pix.source_record_version v
             WHERE v.record_id IN (SELECT record_id FROM pix.link WHERE person_id = $1)
             UNION ALL
             SELECT lower(l.valid), l.made_by, l.reason, 'linked', l.record_id, NULL, l.kind,
                    NULL, NULL, l.id
             FROM pix.link l WHERE l.person_id = $1
             UNION ALL
             SELECT upper(l.valid), NULL, NULL, 'unlinked', l.record_id, NULL, NULL,
                    NULL, NULL, l.id
             FROM pix.link l WHERE l.person_id = $1 AND NOT upper_inf(l.valid)
             UNION ALL
             SELECT m.made_at, m.made_by, m.reason, 'merged', m.id, NULL, m.kind,
                    m.survivor_id, m.merged_id, m.id
             FROM pix.merge m WHERE $1 IN (m.survivor_id, m.merged_id)
             UNION ALL
             SELECT m.reverted_at, m.reverted_by, m.revert_reason, 'unmerged', m.id, NULL, NULL,
                    m.survivor_id, m.merged_id, m.id
             FROM pix.merge m
             WHERE $1 IN (m.survivor_id, m.merged_id) AND m.reverted_at IS NOT NULL
             UNION ALL
             SELECT a.at, a.actor, a.reason, 'action', a.id, NULL, a.kind, NULL, NULL, a.id
             FROM pix.steward_action a WHERE a.person_id = $1
         ) h
         ORDER BY h.at DESC, h.tie DESC
         LIMIT $2"
    )))
    .bind(person)
    .bind(limit)
    .fetch_all(&mut *connection)
    .await?;
    let truncated = rows.len() > HISTORY;
    let mut entries = Vec::with_capacity(rows.len().min(HISTORY));
    for row in rows.into_iter().take(HISTORY) {
        let event = event(&row)?;
        entries.push(HistoryEntry {
            at: row.at,
            actor: row.actor,
            reason: row.reason,
            event,
        });
    }
    Ok((entries, truncated))
}

/// Returns the event one history row records.
fn event(row: &HistoryRow) -> Result<HistoryEvent, OperatorError> {
    let present = |id: Option<Uuid>| {
        id.map(|id| id.to_string())
            .ok_or(OperatorError::Unadmitted("history entry"))
    };
    let detail = || {
        row.detail
            .as_deref()
            .ok_or(OperatorError::Unadmitted("history entry"))
    };
    Ok(match row.event.as_str() {
        "version" => HistoryEvent::Version {
            record: present(row.subject)?,
            version: row
                .version
                .and_then(|version| u32::try_from(version).ok())
                .ok_or(OperatorError::Unadmitted("version"))?,
            channel: channel(detail()?)?,
        },
        "linked" => HistoryEvent::Linked {
            record: present(row.subject)?,
            kind: link_kind(detail()?)?,
        },
        "unlinked" => HistoryEvent::Unlinked {
            record: present(row.subject)?,
        },
        "merged" => HistoryEvent::Merged {
            merge: present(row.subject)?,
            survivor: present(row.survivor)?,
            merged: present(row.merged)?,
        },
        "unmerged" => HistoryEvent::Unmerged {
            merge: present(row.subject)?,
            survivor: present(row.survivor)?,
            merged: present(row.merged)?,
        },
        "action" => HistoryEvent::Action {
            kind: detail()?.to_owned(),
        },
        _ => return Err(OperatorError::Unadmitted("history entry")),
    })
}

/// Reads every pseudonymisation domain with the person's pseudonym in it,
/// masked.
async fn pseudonyms(
    connection: &mut PgConnection,
    person: Uuid,
) -> Result<Vec<PseudonymRow>, OperatorError> {
    let rows: Vec<(Uuid, String, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT d.id, d.namespace, d.name, p.value
         FROM pix.pseudonym_domain d
         LEFT JOIN pix.pseudonym p ON p.domain_id = d.id AND p.person_id = $1
         WHERE d.retired_at IS NULL
         ORDER BY d.namespace
         LIMIT $2",
    )
    .bind(person)
    .bind(PSEUDONYM_DOMAINS)
    .fetch_all(&mut *connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(domain, namespace, name, value)| PseudonymRow {
            domain: domain.to_string(),
            namespace,
            name,
            masked: value.map(|value| Masked(mask(&value.to_string()))),
        })
        .collect())
}

/// Returns the link kind the store spells `stored`.
fn link_kind(stored: &str) -> Result<LinkKind, OperatorError> {
    Ok(match stored {
        "feed" => LinkKind::Feed,
        "deterministic" => LinkKind::Deterministic,
        "probabilistic" => LinkKind::Probabilistic,
        "steward" => LinkKind::Steward,
        "source" => LinkKind::Source,
        "merge" => LinkKind::Merge,
        _ => return Err(OperatorError::Unadmitted("link kind")),
    })
}
