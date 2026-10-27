// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The request a server side received, as its audit records name it,
//! feature `balp` with `pixm` or `pdqm`.
//!
//! The PIXm Manager and the PDQm Supplier record each exchange from the side
//! the request flows to: the client is the agent it flows from and the
//! server's base URL the agent it flows to, as each server-side audit
//! profile fixes its source and destination agents. A server that names each
//! request by an id records the id as an entity of type
//! [`ihe_atna::balp::REQUEST_ID`]; no specification governs that
//! entity: our own design. [`Received`] holds these facts once per request,
//! and each server-side builder of `pixm::audit` and `pdqm::audit` turns
//! them into the record its audit profile fixes.

use ihe_atna::balp::{
    Described, Direction, Entity, EventKind, Exchange, NetworkAddress, Outcome, Peer, REQUEST_ID,
    What,
};
use ihe_atna::user::OnBehalfOf;
use jiff::Timestamp;

/// Who sent a request to the server, where it arrived and for whom it was
/// made.
#[derive(Debug, Clone)]
pub struct Received {
    client: Peer,
    endpoint: NetworkAddress,
    on_behalf: OnBehalfOf,
    request_id: Option<String>,
}

impl Received {
    /// Returns the request `client` sent to the server's `endpoint`, made
    /// for `on_behalf`.
    #[must_use]
    pub fn new(client: Peer, endpoint: NetworkAddress, on_behalf: OnBehalfOf) -> Self {
        Self {
            client,
            endpoint,
            on_behalf,
            request_id: None,
        }
    }

    /// Returns the request named by the server's request id `id`, recorded
    /// as the transaction entity.
    #[must_use]
    pub fn with_request_id(mut self, id: Option<String>) -> Self {
        self.request_id = id;
        self
    }

    /// Returns the record of this request as `kind` fixes it, ended in
    /// `outcome`, concerning `entities` and, when the request has an id, its
    /// transaction entity.
    #[must_use]
    pub fn exchange(
        &self,
        kind: EventKind,
        outcome: Outcome,
        mut entities: Vec<Entity>,
    ) -> Exchange {
        if let Some(id) = &self.request_id {
            entities.push(Entity::Described(Described {
                what: Some(What::Identifier {
                    system: None,
                    value: id.clone(),
                }),
                kind: REQUEST_ID,
                role: None,
                name: None,
                description: None,
                details: Vec::new(),
                security_labels: Vec::new(),
            }));
        }
        Exchange {
            kind,
            recorded: Timestamp::now(),
            outcome,
            outcome_description: None,
            direction: Direction::Received {
                client: self.client.clone(),
                endpoint: self.endpoint.clone(),
            },
            on_behalf: self.on_behalf.clone(),
            entities,
            security: Vec::new(),
        }
    }
}
