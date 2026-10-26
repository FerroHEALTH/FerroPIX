// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A steward's merge and its revert, and the preview of either
//! (`docs/architecture.md` §4.3, §12.3 and §12.4).
//!
//! The preview answers the whole effect and writes nothing, and records a
//! Patient Read of each person it shows. A merge and a revert carry the
//! versions the steward read and a reason; each is one steward action and
//! one BALP Patient Update of the person it leaves standing, written before
//! the transaction commits. A revert that is not of the survivor's latest
//! merge is refused `409` and names the merge to revert first. No
//! specification governs the screens: our own design; PMIR 1.6.0
//! §2:3.93.4.1.3 admits the local unmerge policy.

use axum::extract::Request;
use axum::response::Response;
use ferropix_operator_api::candidate::PersonIdentifier;
use ferropix_operator_api::merge::{
    NewMerge, Outcome as MergeOutcome, Party, Preview, PreviewQuery, PseudonymMove, Revert,
};
use ferropix_operator_api::patient::{DomainRef, Masked};
use http::StatusCode;
use http::request::Parts;
use ihe_atna::balp::Outcome;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::audit::records::{self, operator::StewardAction};
use crate::feed::Context;
use crate::feed::merging::{self, Change, MergeRequest, RevertError};
use crate::feed::steward::DecideError;
use crate::fhir::outcome::IssueType;
use crate::matching::scorer::mask;
use crate::operator::person::{UTC, carried, records as person_records};
use crate::operator::{Operator, OperatorError, demographics, domain_kind, opened, snapshot, wire};
use crate::store::record::{Channel, PersonId};
use crate::store::seal::Sealer;

/// The diagnostics of an id that names no person or no merge.
pub const NOT_FOUND: &str = "no person or merge has this id";

/// The diagnostics of a change made on what moved since it was read.
pub const STALE: &str =
    "a person changed or the merge was reverted since it was read; reload the preview";

/// The diagnostics of a preview or a merge that does not name two
/// different persons, or names a merge too.
pub const NOT_TWO: &str = "a merge names two different persons, survivor and merged, and a revert preview names one merge";

/// The diagnostics of a revert of a merge that is not the survivor's latest.
pub const NOT_LATEST: &str =
    "a later merge of the survivor has to be reverted first; the revert preview names it";

/// The diagnostics of a change without a reason, or one too long.
pub const NO_REASON: &str = "a merge and a revert need a reason of at most 500 characters";

/// The longest reason a change takes, in characters.
const REASON: usize = 500;

/// Answers the preview of the merge or the revert the query names.
pub async fn preview(operator: &Operator, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let query: PreviewQuery = match wire::query(&parts) {
        Ok(query) => query,
        Err(refused) => return *refused,
    };
    let ids = |text: &Option<String>| text.as_deref().map(wire::uuid);
    let read = match (ids(&query.survivor), ids(&query.merged), ids(&query.revert)) {
        (Some(Some(survivor)), Some(Some(merged)), None) if survivor != merged => {
            read_merge(operator, survivor, merged).await
        }
        (None, None, Some(Some(merge))) => read_revert(operator, merge).await,
        _ => return wire::refuse(StatusCode::BAD_REQUEST, IssueType::Invalid, NOT_TWO),
    };
    let preview = match read {
        Ok(Some(preview)) => preview,
        Ok(None) => return not_found(),
        Err(error) => return wire::failed(&error),
    };
    let call = wire::call(operator, &parts);
    let exchanges = [&preview.survivor.id, &preview.merged.id]
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
    wire::answer(&preview)
}

/// Merges the two persons the request posts.
pub async fn create(operator: &Operator, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let (posted, _bytes) = match wire::posted::<NewMerge>(&parts, body).await {
        Ok(posted) => posted,
        Err(refused) => return *refused,
    };
    let reason = match reason(&posted.reason) {
        Ok(reason) => reason,
        Err(refused) => return *refused,
    };
    let (Some(survivor), Some(merged)) = (wire::uuid(&posted.survivor), wire::uuid(&posted.merged))
    else {
        return wire::refuse(StatusCode::BAD_REQUEST, IssueType::Invalid, NOT_TWO);
    };
    if survivor == merged {
        return wire::refuse(StatusCode::BAD_REQUEST, IssueType::Invalid, NOT_TWO);
    }
    let merge_request = MergeRequest {
        survivor: PersonId::from_uuid(survivor),
        survivor_version: posted.survivor_version,
        merged: PersonId::from_uuid(merged),
        merged_version: posted.merged_version,
        reason,
    };
    let made = merging::merge(&operator.store, &merge_request, &context(&parts)).await;
    let (transaction, change) = match made {
        Ok(made) => made,
        Err(error) => return refused(&error),
    };
    finish(operator, &parts, transaction, &change, change.survivor).await
}

/// Reverts the merge the matched route's `{id}` names.
pub async fn revert(operator: &Operator, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let Some(merge) = wire::path_id(&parts) else {
        return not_found();
    };
    let (posted, _bytes) = match wire::posted::<Revert>(&parts, body).await {
        Ok(posted) => posted,
        Err(refused) => return *refused,
    };
    let reason = match reason(&posted.reason) {
        Ok(reason) => reason,
        Err(refused) => return *refused,
    };
    let made = merging::revert(
        &operator.store,
        merge,
        &posted.survivor_version,
        &reason,
        &context(&parts),
    )
    .await;
    let (transaction, change) = match made {
        Ok(made) => made,
        Err(RevertError::NotLatest { .. }) => {
            return wire::refuse(StatusCode::CONFLICT, IssueType::Conflict, NOT_LATEST);
        }
        Err(RevertError::Decide(error)) => return refused(&error),
    };
    finish(operator, &parts, transaction, &change, change.merged).await
}

/// Writes the BALP record of `change` on `person`, commits, and answers.
async fn finish(
    operator: &Operator,
    parts: &Parts,
    transaction: sqlx::Transaction<'static, sqlx::Postgres>,
    change: &Change,
    person: PersonId,
) -> Response {
    let call = wire::call(operator, parts);
    let exchange = records::operator::steward(
        &call,
        StewardAction::Decide,
        &format!("Patient/{}", person.uuid()),
        Outcome::Success,
    );
    if let Err(error) = wire::record(operator, parts, vec![exchange]).await {
        return wire::unaudited(&error);
    }
    if let Err(error) = transaction.commit().await {
        return wire::failed(&error.into());
    }
    operator.store.announced();
    tracing::info!(
        moved = change.moved,
        "a steward merged or unmerged two persons"
    );
    wire::answer(&MergeOutcome {
        merge: change.merge.to_string(),
        action: change.action.to_string(),
        survivor: change.survivor.uuid().to_string(),
        merged: change.merged.uuid().to_string(),
        moved: change.moved,
    })
}

/// The actor, channel and request of `parts`.
fn context(parts: &Parts) -> Context {
    Context {
        actor: wire::actor(parts),
        channel: Channel::Fhir,
        request_id: wire::request_id(parts),
    }
}

/// Returns the trimmed `posted` reason, refusing a blank or a long one.
fn reason(posted: &str) -> Result<String, Box<Response>> {
    let reason = posted.trim();
    if reason.is_empty() || reason.chars().count() > REASON {
        return Err(Box::new(wire::refuse(
            StatusCode::BAD_REQUEST,
            IssueType::Invalid,
            NO_REASON,
        )));
    }
    Ok(reason.to_owned())
}

/// The answer of a change the store did not make.
fn refused(error: &DecideError) -> Response {
    match error {
        DecideError::NotFound => not_found(),
        DecideError::Stale => wire::refuse(StatusCode::CONFLICT, IssueType::Conflict, STALE),
        DecideError::Feed(error) => {
            tracing::error!(error = %crate::chain(error), "a merge or a revert could not be made");
            wire::failed(&OperatorError::Unadmitted("merge"))
        }
    }
}

/// Returns the `404` of an id that names no person or merge.
fn not_found() -> Response {
    wire::refuse(StatusCode::NOT_FOUND, IssueType::NotFound, NOT_FOUND)
}

/// Reads the preview of merging `merged` into `survivor`, or `None` when
/// either is unknown.
async fn read_merge(
    operator: &Operator,
    survivor: Uuid,
    merged: Uuid,
) -> Result<Option<Preview>, OperatorError> {
    let mut snapshot = snapshot(&operator.store).await?;
    let (Some(survivor_party), Some(merged_party)) = (
        party(&mut snapshot, survivor).await?,
        party(&mut snapshot, merged).await?,
    ) else {
        return Ok(None);
    };
    let sealer = operator.store.sealer();
    let identifiers = identifiers_of(&mut snapshot, sealer, merged).await?;
    let pseudonyms = pseudonyms_of(&mut snapshot, merged).await?;
    let candidates: Vec<Uuid> = sqlx::query_scalar(
        "SELECT c.id FROM pix.candidate c
         JOIN pix.link la ON la.record_id = c.record_a AND upper_inf(la.valid)
         JOIN pix.link lb ON lb.record_id = c.record_b AND upper_inf(lb.valid)
         WHERE c.status IN ('open', 'deferred')
           AND ((la.person_id = $1 AND lb.person_id = $2) OR (la.person_id = $2 AND lb.person_id = $1))
         ORDER BY c.id",
    )
    .bind(survivor)
    .bind(merged)
    .fetch_all(&mut *snapshot)
    .await?;
    Ok(Some(Preview {
        survivor: survivor_party,
        merged: merged_party,
        revert: None,
        reason: None,
        first_revert: None,
        identifiers,
        pseudonyms,
        subscriptions: subscriptions(&mut snapshot).await?,
        candidates: candidates.iter().map(Uuid::to_string).collect(),
    }))
}

/// Reads the preview of reverting the merge `merge`, or `None` when the
/// store holds no such merge.
async fn read_revert(operator: &Operator, merge: Uuid) -> Result<Option<Preview>, OperatorError> {
    let mut snapshot = snapshot(&operator.store).await?;
    let row: Option<(Uuid, Uuid, Option<String>)> =
        sqlx::query_as("SELECT survivor_id, merged_id, reason FROM pix.merge WHERE id = $1")
            .bind(merge)
            .fetch_optional(&mut *snapshot)
            .await?;
    let Some((survivor, merged, reason)) = row else {
        return Ok(None);
    };
    let (Some(survivor_party), Some(merged_party)) = (
        party(&mut snapshot, survivor).await?,
        party(&mut snapshot, merged).await?,
    ) else {
        return Ok(None);
    };
    let moved: Vec<Uuid> = sqlx::query_scalar(
        "SELECT record_id FROM pix.merge_effect WHERE merge_id = $1 ORDER BY record_id",
    )
    .bind(merge)
    .fetch_all(&mut *snapshot)
    .await?;
    let records = records_of(&mut snapshot, operator.store.sealer(), &moved).await?;
    let first_revert = merging::latest(&mut snapshot, PersonId::from_uuid(survivor))
        .await?
        .filter(|latest| *latest != merge);
    Ok(Some(Preview {
        survivor: survivor_party,
        merged: merged_party,
        revert: Some(merge.to_string()),
        reason,
        first_revert: first_revert.map(|merge| merge.to_string()),
        identifiers: records,
        pseudonyms: pseudonyms_of(&mut snapshot, merged).await?,
        subscriptions: subscriptions(&mut snapshot).await?,
        candidates: Vec::new(),
    }))
}

/// Reads `person` as a party of a merge, or `None` when it is unknown.
async fn party(
    connection: &mut PgConnection,
    person: Uuid,
) -> Result<Option<Party>, OperatorError> {
    let Some(version) = merging::version(connection, PersonId::from_uuid(person))
        .await
        .map_err(|_error| OperatorError::Unadmitted("person version"))?
    else {
        return Ok(None);
    };
    let row: (String, Option<String>) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT to_char(p.created_at AT TIME ZONE 'UTC', '{UTC}'),
                (SELECT v.demographics::text
                 FROM pix.link l
                 JOIN pix.source_record r ON r.id = l.record_id
                 JOIN pix.source_record_version v ON v.record_id = r.id AND v.version = r.head_version
                 WHERE l.person_id = p.id AND upper_inf(l.valid)
                 ORDER BY v.received_at DESC LIMIT 1)
         FROM pix.person p WHERE p.id = $1"
    )))
    .bind(person)
    .fetch_one(&mut *connection)
    .await?;
    let name = match row.1 {
        Some(stored) => demographics::stored(&stored)?.names.first().map(|name| {
            let mut words: Vec<&str> = name.given.iter().map(String::as_str).collect();
            words.extend(name.family.as_deref());
            words.join(" ")
        }),
        None => None,
    };
    Ok(Some(Party {
        id: person.to_string(),
        version,
        name: name.filter(|name| !name.is_empty()),
        created_at: row.0,
    }))
}

/// Reads every identifier of `person`, masked.
async fn identifiers_of(
    connection: &mut PgConnection,
    sealer: &Sealer,
    person: Uuid,
) -> Result<Vec<PersonIdentifier>, OperatorError> {
    let (own, _records) = person_records(connection, sealer, person).await?;
    let carried = carried(connection, sealer, person).await?;
    Ok(own
        .into_iter()
        .filter(|row| row.link.is_some())
        .chain(carried)
        .map(|row| PersonIdentifier {
            domain: row.domain,
            masked: row.masked,
        })
        .collect())
}

/// One record's identifier as the store holds it.
type RecordIdentifier = (
    Uuid,
    String,
    String,
    String,
    Option<Vec<u8>>,
    Option<String>,
);

/// Reads the identifiers of the records `ids`, masked.
async fn records_of(
    connection: &mut PgConnection,
    sealer: &Sealer,
    ids: &[Uuid],
) -> Result<Vec<PersonIdentifier>, OperatorError> {
    let rows: Vec<RecordIdentifier> = sqlx::query_as(
        "SELECT d.id, d.system, d.name, d.kind, r.identifier_sealed, r.identifier_value
         FROM pix.source_record r JOIN pix.identifier_domain d ON d.id = r.domain_id
         WHERE r.id = ANY($1) ORDER BY d.system, r.id",
    )
    .bind(ids)
    .fetch_all(&mut *connection)
    .await?;
    rows.into_iter()
        .map(|(domain, system, name, kind, sealed, plain)| {
            let value = opened(sealer, domain, sealed.as_deref(), plain.as_deref())?;
            Ok(PersonIdentifier {
                domain: DomainRef {
                    id: domain.to_string(),
                    system,
                    name,
                    kind: domain_kind(&kind)?,
                },
                masked: Masked(mask(value.expose())),
            })
        })
        .collect()
}

/// Reads the pseudonyms `person` holds, masked.
async fn pseudonyms_of(
    connection: &mut PgConnection,
    person: Uuid,
) -> Result<Vec<PseudonymMove>, OperatorError> {
    let rows: Vec<(String, String, Uuid)> = sqlx::query_as(
        "SELECT d.namespace, d.name, p.value
         FROM pix.pseudonym p JOIN pix.pseudonym_domain d ON d.id = p.domain_id
         WHERE p.person_id = $1 ORDER BY d.namespace",
    )
    .bind(person)
    .fetch_all(&mut *connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(namespace, name, value)| PseudonymMove {
            namespace,
            name,
            masked: Masked(mask(&value.to_string())),
        })
        .collect())
}

/// Counts the PMIR subscriptions a change is announced to.
async fn subscriptions(connection: &mut PgConnection) -> Result<u64, OperatorError> {
    let active: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pix.subscription WHERE status = 'active'")
            .fetch_one(&mut *connection)
            .await?;
    Ok(active.unsigned_abs())
}
