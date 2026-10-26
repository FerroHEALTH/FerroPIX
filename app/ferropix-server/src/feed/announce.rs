// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-93 messages a change to persons sends, written to the outbox
//! inside the transaction that made the change, so a committed change is
//! never unannounced (PMIR 1.6.0 §2:3.93.4.1.1).
//!
//! Each active subscription whose criteria match a changed person gets one
//! message naming every change of the transaction it matches, in the
//! subscription's payload format (§2:3.94.4.1.2.1.1). A person is matched
//! as the PDQm Supplier serves it. The sender delivers each subscription's
//! messages in order. No specification governs the outbox: our own design.

use fhir_types::r4::patient::Patient;
use ihe_iti::pmir::server::notification::{self, Addressing, Notice};
use ihe_iti::pmir::server::subscription::Criteria;
use ihe_iti::server::format::Format;
use secrecy::SecretString;
use sqlx::PgConnection;
use url::Url;
use uuid::Uuid;

use crate::feed::FeedError;
use crate::pdqm::PdqError;
use crate::pdqm::patient;
use crate::store::outbox::{self, Channel, Message};
use crate::store::record::PersonId;
use crate::store::{Store, subscription};

/// One change a transaction made to a person, as the feed reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The person was created, by its Source's id (an ITI-93 create).
    Created(PersonId),
    /// The person's records or demographics changed.
    Updated(PersonId),
    /// The person was merged into another (§2:3.93.4.1.2.4).
    Merged(PersonId),
    /// The person's master identity was deleted (an ITI-93 delete).
    Deleted(PersonId),
}

impl Change {
    /// The person the change concerns.
    const fn person(self) -> PersonId {
        match self {
            Self::Created(person)
            | Self::Updated(person)
            | Self::Merged(person)
            | Self::Deleted(person) => person,
        }
    }
}

/// Writes the ITI-93 message of `changes` for each active subscription of
/// `store` that matches them, through `connection`, and returns how many
/// messages it wrote; none while PMIR is off.
///
/// # Errors
/// Returns [`FeedError::Store`] when a statement fails or a message cannot
/// be sealed, [`FeedError::Demographics`] when a person's stored
/// demographics do not read, and [`FeedError::Notice`] when a message cannot
/// be encoded.
pub async fn announce(
    connection: &mut PgConnection,
    store: &Store,
    changes: &[Change],
) -> Result<usize, FeedError> {
    let Some(settings) = store.feed() else {
        return Ok(0);
    };
    if changes.is_empty() {
        return Ok(0);
    }
    let subscriptions = subscription::active_in(&mut *connection).await?;
    if subscriptions.is_empty() {
        return Ok(0);
    }
    let mut ids: Vec<Uuid> = changes
        .iter()
        .map(|change| change.person().uuid())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let persons = patient::persons(&mut *connection, &ids)
        .await
        .map_err(feed_error)?;
    let persons: Vec<patient::Person> = persons.into_values().collect();
    let patients = patient::patients(&mut *connection, store.sealer(), &settings.master, &persons)
        .await
        .map_err(feed_error)?;
    let mut written = 0;
    for active in &subscriptions {
        // NOTE: the criteria were read against PMIR 1.6.0 §2:3.94.4.1.2.1.1 when the subscription was stored, so one that no longer reads names no change.
        let Ok(criteria) = Criteria::parse(&active.criteria) else {
            continue;
        };
        let notices: Vec<Notice> = changes
            .iter()
            .filter_map(|change| {
                let served = patients.get(&change.person().uuid())?;
                criteria.matches(served).then(|| notice(*change, served))
            })
            .collect();
        if notices.is_empty() {
            continue;
        }
        let Ok(destination) = Url::parse(&active.endpoint) else {
            continue;
        };
        let format = Format::of_content_type(Some(&active.payload)).unwrap_or(Format::Json);
        let message_id = Uuid::new_v4().to_string();
        let history_id = Uuid::new_v4().to_string();
        let bundle = notification::message(
            &notices,
            &Addressing {
                message_id: &message_id,
                history_id: &history_id,
                registry: &settings.registry,
                destination: &destination,
            },
        );
        let bytes = crate::fhir::wire::encoded(&bundle, format).ok_or(FeedError::Notice)?;
        let payload = String::from_utf8(bytes).map_err(|_not_utf8| FeedError::Notice)?;
        outbox::enqueue(
            &mut *connection,
            store.sealer(),
            &Message {
                channel: Channel::Pmir,
                destination: active.endpoint.clone(),
                media_type: format.media_type(),
                payload: SecretString::from(payload),
                max_attempts: settings.max_attempts,
                subscription: Some(active.id),
            },
        )
        .await?;
        written += 1;
    }
    if written > 0 {
        tracing::info!(messages = written, "ITI-93 messages are queued");
    }
    Ok(written)
}

/// The notice of `change` to the person served as `served`.
fn notice(change: Change, served: &Patient) -> Notice {
    let patient = Box::new(served.clone());
    match change {
        Change::Created(_) => Notice::Created(patient),
        Change::Updated(_) => Notice::Updated(patient),
        Change::Merged(_) => Notice::Merged(patient),
        Change::Deleted(person) => Notice::Deleted(person.uuid().to_string()),
    }
}

/// The feed's error for a read of the served persons.
fn feed_error(error: PdqError) -> FeedError {
    match error {
        PdqError::Store(error) => FeedError::Store(error),
        PdqError::Demographics(error) => FeedError::Demographics(error),
        _ => FeedError::Notice,
    }
}
