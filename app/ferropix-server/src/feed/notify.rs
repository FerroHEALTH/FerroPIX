// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-10 notifications a cross-reference change sends, written to the
//! outbox inside the transaction that made the change, so a committed change
//! is never unannounced.
//!
//! The Manager notifies a consumer "when there is a change in a set of
//! cross-referenced patient identifiers for any of the patient identifiers
//! belonging to Patient Identifier Domains of interest to the consumer"
//! (ITI TF-2 §3.10.4.1.1). Each person a change created or updated is read
//! once, and each consumer holding one of its domains of interest gets one
//! notification with the person's identifiers in those domains
//! (§3.10.4.1.2). A merged person is announced by its survivor's update, and
//! a person whose last record was removed holds no identifier to list, so
//! neither is notified on its own. The sender delivers each consumer's
//! notifications in order. No specification governs the outbox: our own
//! design.

use std::sync::Arc;

use secrecy::SecretString;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::config::section::hl7v2::{Consumer, Notification};
use crate::feed::FeedError;
use crate::feed::announce::Change;
use crate::hl7v2::authority::Authorities;
use crate::hl7v2::notification::{self, Party};
use crate::hl7v2::stamped::Stamped;
use crate::outbox::Wake;
use crate::store::outbox::{self, Channel, Message};
use crate::store::{Store, xref};

/// The media type an ITI-10 notification is stored under.
pub const MEDIA_TYPE: &str = "application/hl7-v2";

/// What a cross-reference change is notified with while consumers are
/// configured.
#[derive(Debug, Clone)]
pub struct Notifying {
    /// The Manager's sending application and facility.
    pub manager: (String, String),
    /// The consumers, in configuration order.
    pub consumers: Arc<[Consumer]>,
    /// The assigning authority of each domain.
    pub authorities: Authorities,
    /// The retry bounds.
    pub notification: Notification,
    /// What wakes the outbox sender.
    pub wake: Wake,
}

/// Writes the ITI-10 notification of `changes` for each consumer of `store`
/// they concern, through `connection`, and returns how many it wrote; none
/// while no consumer is configured.
///
/// # Errors
/// Returns [`FeedError::Store`] when a statement fails or a notification
/// cannot be sealed.
pub async fn notify(
    connection: &mut PgConnection,
    store: &Store,
    changes: &[Change],
) -> Result<usize, FeedError> {
    let Some(settings) = store.consumers() else {
        return Ok(0);
    };
    let mut persons: Vec<Uuid> = changes
        .iter()
        .filter_map(|change| match change {
            Change::Created(person) | Change::Updated(person) => Some(person.uuid()),
            Change::Merged(_) | Change::Deleted(_) => None,
        })
        .collect();
    persons.sort_unstable();
    persons.dedup();
    let (application, facility) = &settings.manager;
    let manager = Party {
        application,
        facility,
    };
    let mut written = 0;
    for person in persons {
        let held = xref::identifiers(&mut *connection, store.sealer(), person).await?;
        for consumer in settings.consumers.iter() {
            let identifiers: Vec<(String, String)> = held
                .iter()
                .filter(|(system, _)| consumer.wants(system))
                .map(|(system, value)| (system.clone(), value.expose().to_owned()))
                .collect();
            // NOTE: ITI TF-2 §3.10.4.1.2, the example of DOM_A: a person with an identifier in one domain of interest is notified with that one.
            if identifiers.is_empty() {
                continue;
            }
            let text = notification::adt_a31(
                manager,
                Party {
                    application: &consumer.application,
                    facility: &consumer.facility,
                },
                &settings.authorities,
                &identifiers,
                &Stamped::now(),
            );
            outbox::enqueue(
                &mut *connection,
                store.sealer(),
                &Message {
                    channel: Channel::Hl7v2,
                    destination: consumer.name.clone(),
                    media_type: MEDIA_TYPE,
                    payload: SecretString::from(text),
                    max_attempts: settings.notification.max_attempts,
                    subscription: None,
                    consumer: Some(consumer.name.clone()),
                },
            )
            .await?;
            written += 1;
        }
    }
    if written > 0 {
        tracing::info!(notifications = written, "ITI-10 notifications are queued");
    }
    Ok(written)
}
