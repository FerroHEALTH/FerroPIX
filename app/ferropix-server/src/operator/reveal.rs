// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! `POST {base}/operator/persons/{id}/reveal`: one identifier or pseudonym
//! of the person unmasked, with the steward's reason.
//!
//! A reveal is a privacy disclosure. The target must belong to the person:
//! a record whose latest link names the person, an authoritative identifier
//! the person's current records carry, or the person's pseudonym in a
//! domain. The `reveal` steward action, naming the person, the record and
//! the domain but never the value, and the BALP disclosure record (BALP 1.1.4
//! §3:5.7.7) are written in one transaction that commits only once the record
//! is stored; the value is answered after the commit, once, `no-store`. No
//! log line, span or error carries the value or the reason. The shown time
//! is capped at 30 seconds. These rules are our own design
//! (`docs/architecture.md` §12.5, decision D10).

use axum::extract::Request;
use axum::response::Response;
use ferropix_operator_api::reveal::{
    DEFAULT_SECONDS, MAX_REASON, MAX_SECONDS, Reveal, Revealed, Target, Value,
};
use http::StatusCode;
use ihe_atna::balp::Outcome;
use uuid::Uuid;

use crate::audit::records::{self, Disclosed};
use crate::fhir::outcome::IssueType;
use crate::operator::{Operator, OperatorError, Recorded, opened, record_action, wire};

/// The diagnostics of a target the person does not hold.
pub const NOT_HELD: &str = "the person holds no identifier or pseudonym under this handle";

/// The diagnostics of a reveal without a reason, or with one too long.
pub const NO_REASON: &str = "a reveal states its reason, in at most 500 characters";

/// What a reveal found to disclose.
struct Found {
    /// The value.
    value: String,
    /// The identifier's domain system, or the pseudonymisation domain's
    /// namespace.
    system: String,
    /// `identifier` or `pseudonym`.
    kind: &'static str,
    /// The record whose identifier it is.
    record: Option<Uuid>,
    /// The identifier domain.
    domain: Option<Uuid>,
    /// The pseudonymisation domain.
    pseudonym_domain: Option<Uuid>,
}

/// Answers a reveal of the person the matched route's `{id}` names.
pub async fn answer(operator: &Operator, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let Some(person) = wire::path_id(&parts) else {
        return not_held();
    };
    let (reveal, _bytes) = match wire::posted::<Reveal>(&parts, body).await {
        Ok(read) => read,
        Err(refused) => return *refused,
    };
    let reason = reveal.reason.trim();
    if reason.is_empty() || reason.chars().count() > MAX_REASON {
        return wire::refuse(StatusCode::BAD_REQUEST, IssueType::Invalid, NO_REASON);
    }
    let found = match find(operator, person, &reveal.target).await {
        Ok(Some(found)) => found,
        Ok(None) => return not_held(),
        Err(error) => return wire::failed(&error),
    };
    let recorded = Recorded {
        kind: "reveal",
        actor: wire::actor(&parts),
        person: Some(person),
        record: found.record,
        domain: found.domain,
        pseudonym_domain: found.pseudonym_domain,
        reason: Some(reason.to_owned()),
        request_id: wire::request_id(&parts),
    };
    let action = match disclose(operator, &parts, person, &found, &recorded, reason).await {
        Ok(action) => action,
        Err(refused) => return *refused,
    };
    wire::answer(&Revealed {
        value: Value(found.value),
        system: found.system,
        seconds: reveal
            .seconds
            .unwrap_or(DEFAULT_SECONDS)
            .clamp(1, MAX_SECONDS),
        action: action.to_string(),
    })
}

/// Writes the steward action and the BALP disclosure record of `found` in
/// one transaction, and returns the action's id.
async fn disclose(
    operator: &Operator,
    parts: &http::request::Parts,
    person: Uuid,
    found: &Found,
    recorded: &Recorded,
    reason: &str,
) -> Result<Uuid, Box<Response>> {
    let failed = |error: OperatorError| Box::new(wire::failed(&error));
    let mut transaction = operator
        .store
        .pool()
        .begin()
        .await
        .map_err(|error| failed(error.into()))?;
    let action = record_action(&mut *transaction, recorded)
        .await
        .map_err(failed)?;
    let call = wire::call(operator, parts);
    let exchange = records::disclosure(
        &call,
        &format!("Patient/{person}"),
        Disclosed {
            kind: found.kind,
            system: &found.system,
            reason,
        },
        Outcome::Success,
    );
    wire::record(operator, parts, vec![exchange])
        .await
        .map_err(|error| Box::new(wire::unaudited(&error)))?;
    transaction
        .commit()
        .await
        .map_err(|error| failed(error.into()))?;
    Ok(action)
}

/// An identifier of the person as stored: domain, system, sealed value,
/// plain value.
type IdentifierRow = (Uuid, String, Option<Vec<u8>>, Option<String>);

/// Returns the identifier `row` holds, opened, of the record `record` when
/// it is a record's own.
fn identifier(
    operator: &Operator,
    (domain, system, sealed, plain): IdentifierRow,
    record: Option<Uuid>,
) -> Result<Found, OperatorError> {
    let value = opened(
        operator.store.sealer(),
        domain,
        sealed.as_deref(),
        plain.as_deref(),
    )?;
    Ok(Found {
        value: value.expose().to_owned(),
        system,
        kind: "identifier",
        record,
        domain: Some(domain),
        pseudonym_domain: None,
    })
}

/// Returns the `404` of a target the person does not hold.
fn not_held() -> Response {
    wire::refuse(StatusCode::NOT_FOUND, IssueType::NotFound, NOT_HELD)
}

/// Finds `target` among what `person` holds, opened.
async fn find(
    operator: &Operator,
    person: Uuid,
    target: &Target,
) -> Result<Option<Found>, OperatorError> {
    let pool = operator.store.pool();
    match target {
        Target::Record(id) => {
            let Some(id) = wire::uuid(id) else {
                return Ok(None);
            };
            let row: Option<IdentifierRow> = sqlx::query_as(
                "WITH latest AS (
                     SELECT DISTINCT ON (l.record_id) l.record_id, l.person_id
                     FROM pix.link l WHERE l.record_id = $2
                     ORDER BY l.record_id, lower(l.valid) DESC, l.made_at DESC
                 )
                 SELECT d.id, d.system, r.identifier_sealed, r.identifier_value
                 FROM latest la
                 JOIN pix.source_record r ON r.id = la.record_id
                 JOIN pix.identifier_domain d ON d.id = r.domain_id
                 WHERE la.person_id = $1 AND d.retired_at IS NULL",
            )
            .bind(person)
            .bind(id)
            .fetch_optional(pool)
            .await?;
            row.map(|row| identifier(operator, row, Some(id)))
                .transpose()
        }
        Target::Carried(id) => {
            let Some(id) = wire::uuid(id) else {
                return Ok(None);
            };
            let row: Option<IdentifierRow> = sqlx::query_as(
                "SELECT d.id, d.system, c.identifier_sealed, c.identifier_value
                 FROM pix.carried_identifier c
                 JOIN pix.source_record r ON r.id = c.record_id AND c.version = r.head_version
                 JOIN pix.link l ON l.record_id = r.id AND upper_inf(l.valid)
                 JOIN pix.identifier_domain d ON d.id = c.domain_id
                 WHERE c.id = $2 AND l.person_id = $1
                   AND r.active AND r.removed_at IS NULL
                   AND d.kind = 'authoritative' AND d.retired_at IS NULL",
            )
            .bind(person)
            .bind(id)
            .fetch_optional(pool)
            .await?;
            row.map(|row| identifier(operator, row, None)).transpose()
        }
        Target::Pseudonym(domain) => {
            let Some(domain) = wire::uuid(domain) else {
                return Ok(None);
            };
            let row: Option<(String, Uuid)> = sqlx::query_as(
                "SELECT d.namespace, p.value
                 FROM pix.pseudonym p
                 JOIN pix.pseudonym_domain d ON d.id = p.domain_id
                 WHERE p.person_id = $1 AND p.domain_id = $2 AND d.retired_at IS NULL",
            )
            .bind(person)
            .bind(domain)
            .fetch_optional(pool)
            .await?;
            Ok(row.map(|(namespace, value)| Found {
                value: value.to_string(),
                system: namespace,
                kind: "pseudonym",
                record: None,
                domain: None,
                pseudonym_domain: Some(domain),
            }))
        }
    }
}
