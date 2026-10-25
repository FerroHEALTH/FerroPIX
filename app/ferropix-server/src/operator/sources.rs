// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! `GET {base}/operator/sources` and `GET {base}/operator/sources/{id}`: the
//! systems allowed to feed records.
//!
//! A source is a FHIR client an `[[auth.issuer]]` lists in its `clients`, a
//! Basic user of the development profile, or an `[[hl7v2.senders]]` entry,
//! so its admission is configuration and is read from the file, never kept
//! twice: FerroPIX keeps no configuration store (FerroUI 0.3.7
//! `design/COMPONENTS.md` rule 13, file mode), and a source is added or
//! retired in the file. The domains a source feeds are read from the
//! domains (one Source per domain, PIXm 3.1.0 §2:3.104.4.1.3), its FHIR
//! activity from the steward actions its feeds recorded and its HL7 v2
//! activity from the receipts of its messages, each row a time, a channel,
//! an action and an outcome, never a value. No specification governs the
//! API: our own design.

use std::collections::BTreeMap;

use axum::extract::{MatchedPath, Request};
use axum::response::Response;
use ferropix_operator_api::patient::Channel;
use ferropix_operator_api::source::{Admission, MESSAGES, Message, Source, SourcePage, Sources};
use http::StatusCode;

use crate::config::section::hl7v2::Sender;
use crate::fhir::outcome::IssueType;
use crate::operator::{Operator, OperatorError, wire};

/// The diagnostics of an id that names no source.
pub const NO_SUCH_SOURCE: &str = "no source has this id";

/// The steward action kinds a FHIR feed records.
const FEED_KINDS: [&str; 3] = ["feed", "resolve-duplicate", "remove"];

/// Answers every source.
pub async fn list(operator: &Operator) -> Response {
    match read_all(operator).await {
        Ok(sources) => wire::answer(&Sources {
            sources,
            file: operator.configured.origins.file().map(str::to_owned),
        }),
        Err(error) => wire::failed(&error),
    }
}

/// Answers one source with its last messages.
pub async fn one(operator: &Operator, request: Request) -> Response {
    let (parts, _body) = request.into_parts();
    let Some(id) = source_id(&parts) else {
        return not_found();
    };
    let sources = match read_all(operator).await {
        Ok(sources) => sources,
        Err(error) => return wire::failed(&error),
    };
    let Some(source) = sources.into_iter().find(|source| source.id == id) else {
        return not_found();
    };
    match messages(operator, &source).await {
        Ok(messages) => wire::answer(&SourcePage { source, messages }),
        Err(error) => wire::failed(&error),
    }
}

/// Returns the `404` of an id that names no source.
fn not_found() -> Response {
    wire::refuse(StatusCode::NOT_FOUND, IssueType::NotFound, NO_SUCH_SOURCE)
}

/// Returns the `{id}` of the route, read from the path by its template.
fn source_id(parts: &http::request::Parts) -> Option<String> {
    let template = parts.extensions.get::<MatchedPath>()?.as_str();
    let position = template.split('/').position(|segment| segment == "{id}")?;
    let path = parts.uri.path();
    if path.split('/').count() != template.split('/').count() {
        return None;
    }
    let id = path.split('/').nth(position)?;
    let decoded = url::form_urlencoded::parse(format!("id={id}").as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())?;
    (!decoded.is_empty()).then_some(decoded)
}

/// Returns the id of the HL7 v2 `sender`: its facility and application
/// joined by `.`.
#[must_use]
pub fn sender_id(sender: &Sender) -> String {
    format!("{}.{}", sender.facility, sender.application)
}

/// Returns the name the face records `sender`'s messages under: its
/// facility and application joined by `|` (ITI TF-2 §3.8.5.1.2).
#[must_use]
pub fn sender_name(sender: &Sender) -> String {
    format!("{}|{}", sender.facility, sender.application)
}

/// Reads every source, ordered by id.
///
/// # Errors
///
/// Returns [`OperatorError`] when the store cannot be read.
pub async fn read_all(operator: &Operator) -> Result<Vec<Source>, OperatorError> {
    let domains = operator.sources.domains();
    let fed_by = |client: &str| -> Vec<String> {
        domains
            .iter()
            .filter(|(_, source)| source.as_str() == client)
            .map(|(system, _)| system.clone())
            .collect()
    };
    let mut sources: BTreeMap<String, Source> = BTreeMap::new();
    for (issuer, clients) in operator.sources.clients() {
        for client in clients {
            sources.entry(client.clone()).or_insert_with(|| Source {
                id: client.clone(),
                name: client.clone(),
                channel: Channel::Fhir,
                admission: Admission::IdentityProvider {
                    issuer: issuer.clone(),
                },
                domains: fed_by(client),
                last_activity: None,
            });
        }
    }
    for user in operator.sources.basic_users() {
        sources.entry(user.clone()).or_insert_with(|| Source {
            id: user.clone(),
            name: user.clone(),
            channel: Channel::Fhir,
            admission: Admission::BasicUser,
            domains: fed_by(user),
            last_activity: None,
        });
    }
    for sender in &operator.configured.senders {
        let id = sender_id(sender);
        sources.entry(id.clone()).or_insert_with(|| Source {
            id,
            name: sender_name(sender),
            channel: Channel::V2,
            admission: Admission::RegisteredSender {
                application: sender.application.clone(),
                facility: sender.facility.clone(),
            },
            domains: Some(sender.domain.clone())
                .filter(|domain| !domain.trim().is_empty())
                .into_iter()
                .collect(),
            last_activity: None,
        });
    }
    let fhir: BTreeMap<String, String> = sqlx::query_as::<_, (String, String)>(
        "SELECT actor, to_char(max(at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
         FROM pix.steward_action WHERE kind = ANY($1) GROUP BY actor",
    )
    .bind(FEED_KINDS.as_slice())
    .fetch_all(operator.store.pool())
    .await?
    .into_iter()
    .collect();
    let v2: BTreeMap<String, String> = sqlx::query_as::<_, (String, String)>(
        "SELECT sending_facility,
                to_char(max(received_at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
         FROM pix.v2_receipt GROUP BY sending_facility",
    )
    .fetch_all(operator.store.pool())
    .await?
    .into_iter()
    .collect();
    for source in sources.values_mut() {
        source.last_activity = match source.channel {
            Channel::V2 => v2.get(&source.name).cloned(),
            Channel::Fhir | Channel::Pmir => fhir.get(&source.name).cloned(),
        };
    }
    Ok(sources.into_values().collect())
}

/// Reads the last messages of `source`, newest first.
async fn messages(operator: &Operator, source: &Source) -> Result<Vec<Message>, OperatorError> {
    let limit = i64::try_from(MESSAGES).unwrap_or(i64::MAX);
    let rows: Vec<(String, String)> =
        match source.channel {
            Channel::V2 => sqlx::query_as(
                "SELECT to_char(received_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
                        ack_code
                 FROM pix.v2_receipt WHERE sending_facility = $1
                 ORDER BY received_at DESC LIMIT $2",
            )
            .bind(&source.name)
            .bind(limit)
            .fetch_all(operator.store.pool())
            .await?,
            Channel::Fhir | Channel::Pmir => {
                sqlx::query_as(
                    "SELECT to_char(at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), kind
                 FROM pix.steward_action WHERE actor = $1 AND kind = ANY($2)
                 ORDER BY at DESC, id DESC LIMIT $3",
                )
                .bind(&source.name)
                .bind(FEED_KINDS.as_slice())
                .bind(limit)
                .fetch_all(operator.store.pool())
                .await?
            }
        };
    Ok(rows
        .into_iter()
        .map(|(at, what)| match source.channel {
            Channel::V2 => Message {
                at,
                channel: Channel::V2,
                action: String::from("message"),
                outcome: what,
            },
            channel => Message {
                at,
                channel,
                action: what,
                outcome: String::from("applied"),
            },
        })
        .collect())
}
