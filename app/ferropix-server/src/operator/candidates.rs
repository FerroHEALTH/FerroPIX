// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The review queue, one candidate with its evidence, and the steward's
//! decision (`docs/architecture.md` §5.3 and §12.4).
//!
//! The queue lists the open candidates and the deferred ones whose date has
//! come, the grade first, then the score, then the age; reading it records a
//! BALP Patient Query per person it shows, and reading one candidate a
//! Patient Read of each of its two persons. A decision carries the versions
//! the steward read and a reason, and is refused `409` when the candidate
//! was decided or a record moved since; it is one steward action and one
//! BALP Patient Update of the person it leaves the records in (the first
//! record's person for a decision that links nothing), written before it
//! commits. No
//! specification governs the queue: our own design.

use axum::extract::Request;
use axum::response::Response;
use ferropix_operator_api::candidate::{
    Candidate, CandidateQuery, CandidateRecord, CandidateRow, Candidates, Decided, Decision,
    Evidence, PAGE, PersonIdentifier, Side, Status, Thresholds, Verdict,
};
use ferropix_operator_api::patient::{DomainRef, Grade, PersonStatus};
use http::StatusCode;
use http::request::Parts;
use ihe_atna::balp::Outcome;
use secrecy::SecretString;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::audit::records::{self, operator::StewardAction};
use crate::feed::Context;
use crate::feed::steward::{self, DecideError};
use crate::fhir::outcome::IssueType;
use crate::operator::person::{UTC, carried, records as person_records};
use crate::operator::{Operator, OperatorError, demographics, domain_kind, grade, snapshot, wire};
use crate::store::record::Channel;

/// The diagnostics of an id that names no candidate.
pub const NO_SUCH_CANDIDATE: &str = "no candidate has this id";

/// The diagnostics of a decision made on what changed since it was read.
pub const STALE: &str =
    "the candidate was decided or a record changed since it was read; reload it";

/// The diagnostics of a decision without a reason, or one too long.
pub const NO_REASON: &str = "a decision needs a reason of at most 500 characters";

/// The diagnostics of a deferral without a day after today.
pub const NO_DAY: &str =
    "a deferral needs a day after today as YYYY-MM-DD, and only a deferral takes one";

/// The longest reason a decision takes, in characters.
const REASON: usize = 500;

/// The order of the queue: the grade first, then the score, then the age.
const QUEUE_ORDER: &str =
    "CASE c.grade WHEN 'certain' THEN 3 WHEN 'probable' THEN 2 ELSE 1 END DESC,
     c.score DESC, c.created_at, c.id";

/// Which candidates the queue holds: the open ones and the deferred ones
/// whose date has come.
const IN_QUEUE: &str =
    "(c.status = 'open' OR (c.status = 'deferred' AND c.deferred_until <= now()))";

/// Answers one page of the queue the request's filters admit.
pub async fn list(operator: &Operator, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let query: CandidateQuery = match wire::query(&parts) {
        Ok(query) => query,
        Err(refused) => return *refused,
    };
    let (page, persons) = match read_queue(operator, &query).await {
        Ok(read) => read,
        Err(error) => return wire::failed(&error),
    };
    let references: Vec<String> = persons
        .iter()
        .map(|person| format!("Patient/{person}"))
        .collect();
    let call = wire::call(operator, &parts);
    let exchanges = records::operator::steward_search(
        &call,
        &raw(operator, &parts),
        &references,
        Outcome::Success,
    );
    if let Err(error) = wire::record(operator, &parts, exchanges).await {
        return wire::unaudited(&error);
    }
    wire::answer(&page)
}

/// Answers the candidate the matched route's `{id}` names.
pub async fn one(operator: &Operator, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let Some(id) = wire::path_id(&parts) else {
        return not_found();
    };
    let candidate = match read_one(operator, id).await {
        Ok(Some(candidate)) => candidate,
        Ok(None) => return not_found(),
        Err(error) => return wire::failed(&error),
    };
    let call = wire::call(operator, &parts);
    let mut persons = vec![candidate.a.person.clone(), candidate.b.person.clone()];
    persons.dedup();
    let exchanges = persons
        .iter()
        .map(|person| {
            records::operator::steward(
                &call,
                StewardAction::View,
                &format!("Patient/{person}"),
                Outcome::Success,
            )
        })
        .collect();
    if let Err(error) = wire::record(operator, &parts, exchanges).await {
        return wire::unaudited(&error);
    }
    wire::answer(&candidate)
}

/// Makes the decision the request posts on the candidate the matched
/// route's `{id}` names.
pub async fn decide(operator: &Operator, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let Some(id) = wire::path_id(&parts) else {
        return not_found();
    };
    let (decision, _bytes) = match wire::posted::<Decision>(&parts, body).await {
        Ok(posted) => posted,
        Err(refused) => return *refused,
    };
    let decision = match checked(id, &decision) {
        Ok(decision) => decision,
        Err(refused) => return *refused,
    };
    let context = Context {
        actor: wire::actor(&parts),
        channel: Channel::Fhir,
        request_id: wire::request_id(&parts),
    };
    let (transaction, decided) = match steward::decide(&operator.store, &decision, &context).await {
        Ok(made) => made,
        Err(DecideError::NotFound) => return not_found(),
        Err(DecideError::Stale) => {
            return wire::refuse(StatusCode::CONFLICT, IssueType::Conflict, STALE);
        }
        Err(DecideError::Feed(error)) => {
            return wire::failed(&OperatorError::Unadmitted(feed_error(&error)));
        }
    };
    let call = wire::call(operator, &parts);
    let Some(person) = decided.person.or_else(|| decided.persons.first().copied()) else {
        return wire::failed(&OperatorError::Unadmitted(
            "candidate record without a person",
        ));
    };
    let exchanges = vec![records::operator::steward(
        &call,
        StewardAction::Decide,
        &format!("Patient/{}", person.uuid()),
        Outcome::Success,
    )];
    if let Err(error) = wire::record(operator, &parts, exchanges).await {
        return wire::unaudited(&error);
    }
    if let Err(error) = transaction.commit().await {
        return wire::failed(&error.into());
    }
    operator.store.announced();
    tracing::info!(
        verdict = verdict_name(decision.verdict),
        merged = decided.merge.is_some(),
        "a steward decided a candidate"
    );
    let next = match next_after(operator, id).await {
        Ok(next) => next,
        Err(error) => return wire::failed(&error),
    };
    wire::answer(&Decided {
        candidate: id.to_string(),
        status: match decision.verdict {
            steward::Verdict::Link => Status::Linked,
            steward::Verdict::NotAMatch => Status::NotAMatch,
            steward::Verdict::Defer { .. } => Status::Deferred,
        },
        action: decided.action.to_string(),
        merge: decided.merge.map(|merge| merge.to_string()),
        person: decided.person.map(|person| person.uuid().to_string()),
        next,
    })
}

/// Returns the decision of `posted` on the candidate `id`, refusing a
/// reason that is blank or too long and a deferral without a day after
/// today.
fn checked(id: Uuid, posted: &Decision) -> Result<steward::Decision, Box<Response>> {
    let reason = posted.reason.trim();
    if reason.is_empty() || reason.chars().count() > REASON {
        return Err(Box::new(wire::refuse(
            StatusCode::BAD_REQUEST,
            IssueType::Invalid,
            NO_REASON,
        )));
    }
    let no_day = || {
        Box::new(wire::refuse(
            StatusCode::BAD_REQUEST,
            IssueType::Invalid,
            NO_DAY,
        ))
    };
    let verdict = match (posted.verdict, posted.until.as_deref()) {
        (Verdict::Link, None) => steward::Verdict::Link,
        (Verdict::NotAMatch, None) => steward::Verdict::NotAMatch,
        (Verdict::Defer, Some(until)) => {
            let until: jiff::civil::Date = until.parse().map_err(|_unread| no_day())?;
            let today = jiff::Zoned::now()
                .with_time_zone(jiff::tz::TimeZone::UTC)
                .date();
            if until <= today {
                return Err(no_day());
            }
            steward::Verdict::Defer { until }
        }
        _ => return Err(no_day()),
    };
    Ok(steward::Decision {
        candidate: id,
        verdict,
        a_version: posted.a_version,
        b_version: posted.b_version,
        reason: reason.to_owned(),
    })
}

/// The name a log line gives a verdict.
const fn verdict_name(verdict: steward::Verdict) -> &'static str {
    match verdict {
        steward::Verdict::Link => "link",
        steward::Verdict::NotAMatch => "not-a-match",
        steward::Verdict::Defer { .. } => "defer",
    }
}

/// The kind of error a refused decision's log line names.
fn feed_error(error: &crate::feed::FeedError) -> &'static str {
    tracing::error!(error = %crate::chain(error), "a steward decision could not be made");
    "decision"
}

/// Returns the `404` of an id that names no candidate.
fn not_found() -> Response {
    wire::refuse(
        StatusCode::NOT_FOUND,
        IssueType::NotFound,
        NO_SUCH_CANDIDATE,
    )
}

/// The raw request of a queue read, for its audit record.
fn raw(operator: &Operator, parts: &Parts) -> SecretString {
    let base = operator.endpoint.as_str().trim_end_matches('/');
    let rest = parts
        .uri
        .path_and_query()
        .map_or("/candidates", |path| path.as_str())
        .trim_start_matches("/operator");
    let url =
        url::Url::parse(&format!("{base}{rest}")).unwrap_or_else(|_| operator.endpoint.clone());
    records::raw_request(&url, None)
}

/// One queue row as the store holds it.
#[derive(sqlx::FromRow)]
struct QueueRow {
    id: Uuid,
    score: f64,
    grade: String,
    status: String,
    created_at: String,
    deferred_until: Option<String>,
    a_domain: Uuid,
    a_system: String,
    a_name: String,
    a_kind: String,
    a_demographics: String,
    b_domain: Uuid,
    b_system: String,
    b_name: String,
    b_kind: String,
    b_demographics: String,
    total: i64,
}

/// Reads the page of the queue `query` admits, and the persons it shows.
async fn read_queue(
    operator: &Operator,
    query: &CandidateQuery,
) -> Result<(Candidates, Vec<Uuid>), OperatorError> {
    let mut snapshot = snapshot(&operator.store).await?;
    let which = if query.deferred {
        "(c.status = 'deferred' AND c.deferred_until > now())"
    } else {
        IN_QUEUE
    };
    let rows: Vec<QueueRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT c.id, c.score, c.grade, c.status,
                to_char(c.created_at AT TIME ZONE 'UTC', '{UTC}') AS created_at,
                to_char(c.deferred_until AT TIME ZONE 'UTC', '{UTC}') AS deferred_until,
                da.id AS a_domain, da.system AS a_system, da.name AS a_name, da.kind AS a_kind,
                va.demographics::text AS a_demographics,
                db.id AS b_domain, db.system AS b_system, db.name AS b_name, db.kind AS b_kind,
                vb.demographics::text AS b_demographics,
                count(*) OVER () AS total
         FROM pix.candidate c
         JOIN pix.source_record ra ON ra.id = c.record_a
         JOIN pix.identifier_domain da ON da.id = ra.domain_id
         JOIN pix.source_record_version va ON va.record_id = c.record_a AND va.version = c.a_version
         JOIN pix.source_record rb ON rb.id = c.record_b
         JOIN pix.identifier_domain db ON db.id = rb.domain_id
         JOIN pix.source_record_version vb ON vb.record_id = c.record_b AND vb.version = c.b_version
         WHERE {which}
           AND ($1::text IS NULL OR c.grade = $1)
           AND ($2::text IS NULL OR da.system = $2 OR db.system = $2)
           AND ($3::integer IS NULL OR c.created_at <= now() - make_interval(days => $3))
         ORDER BY {QUEUE_ORDER}
         LIMIT $4 OFFSET $5"
    )))
    .bind(query.grade.map(grade_code))
    .bind(query.domain.as_deref())
    .bind(
        query
            .older_than_days
            .and_then(|days| i32::try_from(days).ok()),
    )
    .bind(i64::from(PAGE))
    .bind(i64::from(query.offset))
    .fetch_all(&mut *snapshot)
    .await?;
    let total = rows
        .first()
        .map_or(0, |row| crate::operator::count(row.total));
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let persons: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT l.person_id
         FROM pix.candidate c
         JOIN pix.link l ON l.record_id IN (c.record_a, c.record_b) AND upper_inf(l.valid)
         WHERE c.id = ANY($1)
         ORDER BY l.person_id",
    )
    .bind(&ids)
    .fetch_all(&mut *snapshot)
    .await?;
    let last_decision_at: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT to_char(max(decided_at) AT TIME ZONE 'UTC', '{UTC}') FROM pix.candidate"
    )))
    .fetch_one(&mut *snapshot)
    .await?;
    let mut candidates = Vec::with_capacity(rows.len());
    for row in rows {
        candidates.push(CandidateRow {
            id: row.id.to_string(),
            a: side(
                row.a_domain,
                row.a_system,
                row.a_name,
                &row.a_kind,
                &row.a_demographics,
            )?,
            b: side(
                row.b_domain,
                row.b_system,
                row.b_name,
                &row.b_kind,
                &row.b_demographics,
            )?,
            score: row.score,
            grade: grade(&row.grade)?,
            status: status(&row.status)?,
            created_at: row.created_at,
            deferred_until: row.deferred_until,
        });
    }
    let shown = u64::try_from(candidates.len()).unwrap_or(u64::MAX);
    let next = (u64::from(query.offset).saturating_add(shown) < total)
        .then(|| query.offset.saturating_add(PAGE));
    Ok((
        Candidates {
            total,
            offset: query.offset,
            next,
            rows: candidates,
            last_decision_at,
        },
        persons,
    ))
}

/// Returns one side of a queue row.
fn side(
    domain: Uuid,
    system: String,
    name: String,
    kind: &str,
    stored: &str,
) -> Result<Side, OperatorError> {
    let demographics = demographics::stored(stored)?;
    let leading = demographics.names.first().map(|name| {
        let mut words: Vec<&str> = name.given.iter().map(String::as_str).collect();
        words.extend(name.family.as_deref());
        words.join(" ")
    });
    Ok(Side {
        domain: DomainRef {
            id: domain.to_string(),
            system,
            name,
            kind: domain_kind(kind)?,
        },
        name: leading.filter(|name| !name.is_empty()),
        birth_date: demographics.birth_date,
    })
}

/// One candidate as the store holds it.
#[derive(sqlx::FromRow)]
struct CandidateFields {
    record_a: Uuid,
    record_b: Uuid,
    a_version: i32,
    b_version: i32,
    score: f64,
    grade: String,
    evidence: String,
    status: String,
    created_at: String,
    decided_by: Option<String>,
    decided_at: Option<String>,
    reason: Option<String>,
    deferred_until: Option<String>,
}

/// Reads the candidate `id`, or `None` when the store holds none.
async fn read_one(operator: &Operator, id: Uuid) -> Result<Option<Candidate>, OperatorError> {
    let mut snapshot = snapshot(&operator.store).await?;
    let fields: Option<CandidateFields> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT record_a, record_b, a_version, b_version, score, grade, evidence::text AS evidence,
                status, to_char(created_at AT TIME ZONE 'UTC', '{UTC}') AS created_at,
                decided_by, to_char(decided_at AT TIME ZONE 'UTC', '{UTC}') AS decided_at, reason,
                to_char(deferred_until AT TIME ZONE 'UTC', '{UTC}') AS deferred_until
         FROM pix.candidate WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(&mut *snapshot)
    .await?;
    let Some(fields) = fields else {
        return Ok(None);
    };
    let sealer = operator.store.sealer();
    let a = record(&mut snapshot, sealer, fields.record_a, fields.a_version).await?;
    let b = record(&mut snapshot, sealer, fields.record_b, fields.b_version).await?;
    let (previous, next) = neighbours(&mut snapshot, id).await?;
    let merges = a.person != b.person;
    Ok(Some(Candidate {
        id: id.to_string(),
        status: status(&fields.status)?,
        score: fields.score,
        grade: grade(&fields.grade)?,
        evidence: evidence(&fields.evidence)?,
        a,
        b,
        thresholds: Thresholds {
            certain: operator.matching.link_at,
            probable: operator.matching.review_at,
            possible: operator.matching.possible_at,
        },
        merges,
        created_at: fields.created_at,
        decided_by: fields.decided_by,
        decided_at: fields.decided_at,
        reason: fields.reason,
        deferred_until: fields.deferred_until,
        next,
        previous,
    }))
}

/// One record of a candidate as the store holds it.
#[derive(sqlx::FromRow)]
struct RecordFields {
    head_version: i32,
    domain_id: Uuid,
    system: String,
    name: String,
    kind: String,
    demographics: String,
    person: Uuid,
    person_status: String,
}

/// Reads the record `id` at `version`, its person and that person's
/// identifiers, masked.
async fn record(
    connection: &mut PgConnection,
    sealer: &crate::store::seal::Sealer,
    id: Uuid,
    version: i32,
) -> Result<CandidateRecord, OperatorError> {
    let fields: RecordFields = sqlx::query_as(
        "SELECT r.head_version, d.id AS domain_id, d.system, d.name, d.kind,
                v.demographics::text AS demographics,
                p.id AS person, p.status AS person_status
         FROM pix.source_record r
         JOIN pix.identifier_domain d ON d.id = r.domain_id
         JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = $2
         JOIN LATERAL (
             SELECT l.person_id FROM pix.link l WHERE l.record_id = r.id
             ORDER BY upper_inf(l.valid) DESC, lower(l.valid) DESC LIMIT 1
         ) latest ON true
         JOIN pix.person p ON p.id = latest.person_id
         WHERE r.id = $1",
    )
    .bind(id)
    .bind(version)
    .fetch_one(&mut *connection)
    .await?;
    let (own, _records) = person_records(connection, sealer, fields.person).await?;
    let carried = carried(connection, sealer, fields.person).await?;
    let identifiers = own
        .into_iter()
        .chain(carried)
        .map(|row| PersonIdentifier {
            domain: row.domain,
            masked: row.masked,
        })
        .collect();
    Ok(CandidateRecord {
        id: id.to_string(),
        version,
        head_version: fields.head_version,
        domain: DomainRef {
            id: fields.domain_id.to_string(),
            system: fields.system,
            name: fields.name,
            kind: domain_kind(&fields.kind)?,
        },
        demographics: demographics::stored(&fields.demographics)?,
        person: fields.person.to_string(),
        person_status: match fields.person_status.as_str() {
            "active" => PersonStatus::Active,
            "merged" => PersonStatus::Merged,
            _ => return Err(OperatorError::Unadmitted("person status")),
        },
        identifiers,
    })
}

/// Returns the candidates before and after `id` in the queue's order.
async fn neighbours(
    connection: &mut PgConnection,
    id: Uuid,
) -> Result<(Option<String>, Option<String>), OperatorError> {
    let row: Option<(Option<Uuid>, Option<Uuid>)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT previous, next FROM (
             SELECT c.id,
                    lag(c.id) OVER (ORDER BY {QUEUE_ORDER}) AS previous,
                    lead(c.id) OVER (ORDER BY {QUEUE_ORDER}) AS next
             FROM pix.candidate c WHERE {IN_QUEUE} OR c.id = $1
         ) ordered WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?;
    let (previous, next) = row.unwrap_or((None, None));
    Ok((
        previous.map(|id| id.to_string()),
        next.map(|id| id.to_string()),
    ))
}

/// Returns the first candidate in the queue's order once `id` was decided.
async fn next_after(operator: &Operator, id: Uuid) -> Result<Option<String>, OperatorError> {
    let mut snapshot = snapshot(&operator.store).await?;
    let (_previous, next) = neighbours(&mut snapshot, id).await?;
    if next.is_some() {
        return Ok(next);
    }
    let first: Option<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT c.id FROM pix.candidate c WHERE {IN_QUEUE} ORDER BY {QUEUE_ORDER} LIMIT 1"
    )))
    .fetch_optional(&mut *snapshot)
    .await?;
    Ok(first.map(|id| id.to_string()))
}

/// Returns the status the store spells `stored`.
fn status(stored: &str) -> Result<Status, OperatorError> {
    match stored {
        "open" => Ok(Status::Open),
        "linked" => Ok(Status::Linked),
        "not-a-match" => Ok(Status::NotAMatch),
        "deferred" => Ok(Status::Deferred),
        _ => Err(OperatorError::Unadmitted("candidate status")),
    }
}

/// Returns the store's spelling of `grade`.
const fn grade_code(grade: Grade) -> &'static str {
    match grade {
        Grade::Certain => "certain",
        Grade::Probable => "probable",
        Grade::Possible => "possible",
        Grade::CertainlyNot => "certainly-not",
    }
}

/// Reads the stored evidence as the document's.
fn evidence(stored: &str) -> Result<Evidence, OperatorError> {
    serde_json::from_str(stored).map_err(|_unread| OperatorError::Unadmitted("candidate evidence"))
}

#[cfg(test)]
mod tests {
    use ferropix_operator_api::candidate::Agreement;

    use super::evidence;

    #[test]
    fn the_scorer_s_evidence_reads_as_the_document_s() {
        let matching = crate::config::section::matching::Matching::default();
        let scorer = crate::matching::scorer::Scorer::new(&matching);
        let patient = |family: &str| {
            let text = format!(
                r#"{{"resourceType":"Patient","name":[{{"family":"{family}","given":["Ada"]}}],"birthDate":"1980-02-29","gender":"female"}}"#
            );
            crate::feed::demographics::patient(&text).expect("a patient")
        };
        let a = crate::matching::subject::Subject::from_patient(
            &patient("Proefsma"),
            std::collections::BTreeSet::new(),
        );
        let b = crate::matching::subject::Subject::from_patient(
            &patient("Proefsmaa"),
            std::collections::BTreeSet::new(),
        );
        let found = scorer.score(&a, &b, None);
        let stored = serde_json::to_string(&found.evidence).expect("the evidence writes");
        let read = evidence(&stored).expect("the stored evidence reads as the document's");
        assert_eq!(read.fields.len(), found.evidence.fields.len());
        assert!(
            read.fields
                .iter()
                .any(|field| field.field == "family_name" && field.agreement != Agreement::Absent)
        );
    }
}
