// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-55 Initiating Gateway audit message (ITI TF-2 §3.55.5.1.1), handed
//! to a seam a deployment routes to its audit repository.
//!
//! [`XcpdClient::audited`](super::XcpdClient::audited) gives the client an
//! [`AuditRecorder`]; after every exchange it records one [`AuditEvent`]
//! with the fields the transaction's audit table fills: the event (a DICOM
//! `Query`, action `E`, the `ITI-55` event type), the outcome, the source's
//! process id, the destination's SOAP endpoint and network access point, and
//! the query parameters with the `homeCommunityID` detail. Encoding it as a
//! DICOM PS3.15 `AuditMessage` and sending it to an ATNA Audit Record
//! Repository is the recorder's work.
//!
//! The query parameters are the `QueryByParameter` segment of the request,
//! which names the patient identifier, as the audit table requires: the event
//! holds them as a [`SecretString`], its `Debug` redacts them, and a recorder
//! that logs the event must leave them out.

use std::fmt;
use std::net::IpAddr;

use jiff::Timestamp;
use secrecy::SecretString;
use url::Url;

use super::identifier::HomeCommunityId;
use crate::redact::{REDACTED, RedactedUrl};

/// The DICOM `EventID` of the message: `EV(110112, DCM, "Query")`.
pub const EVENT_ID: (&str, &str, &str) = ("110112", "DCM", "Query");

/// The `EventActionCode` of the message: `E`, execute.
pub const EVENT_ACTION: &str = "E";

/// The `EventTypeCode` of the message:
/// `EV("ITI-55", "IHE Transactions", "Cross Gateway Patient Discovery")`.
pub const EVENT_TYPE: (&str, &str, &str) = (
    "ITI-55",
    "IHE Transactions",
    "Cross Gateway Patient Discovery",
);

/// The `ParticipantObjectDetail` type that carries the `homeCommunityID`.
pub const HOME_COMMUNITY_DETAIL: &str = "ihe:homeCommunityID";

/// How an exchange ended, as the `EventOutcomeIndicator` codes it (DICOM
/// PS3.15 Annex A.5).
///
/// The codes leave the meaning of a failure to the reporting application;
/// here the gateway's answer and its absence are told apart (no
/// specification governs the choice: our own design).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EventOutcome {
    /// `0`, nominal success: the answer was read as a discovery.
    Success,
    /// `4`, minor failure: the responding gateway answered with a failure,
    /// or with an answer that does not hold to ITI-55.
    MinorFailure,
    /// `8`, serious failure: no answer arrived.
    SeriousFailure,
}

impl EventOutcome {
    /// The `EventOutcomeIndicator` value.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Success => "0",
            Self::MinorFailure => "4",
            Self::SeriousFailure => "8",
        }
    }
}

/// A `NetworkAccessPointID` with its type code: `1` for a machine (DNS)
/// name, `2` for an IP address (§3.55.5.1.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkAccessPoint {
    /// A machine (DNS) name, type code `1`.
    MachineName(String),
    /// An IP address, type code `2`.
    IpAddress(IpAddr),
}

impl NetworkAccessPoint {
    /// The access point of `url`'s host, `None` when it has none.
    #[must_use]
    pub fn of(url: &Url) -> Option<Self> {
        match url.host()? {
            url::Host::Domain(name) => Some(Self::MachineName(name.to_owned())),
            url::Host::Ipv4(address) => Some(Self::IpAddress(IpAddr::V4(address))),
            url::Host::Ipv6(address) => Some(Self::IpAddress(IpAddr::V6(address))),
        }
    }

    /// The `NetworkAccessPointTypeCode`.
    #[must_use]
    pub fn type_code(&self) -> &'static str {
        match self {
            Self::MachineName(_) => "1",
            Self::IpAddress(_) => "2",
        }
    }

    /// The `NetworkAccessPointID`.
    #[must_use]
    pub fn id(&self) -> String {
        match self {
            Self::MachineName(name) => name.clone(),
            Self::IpAddress(address) => address.to_string(),
        }
    }
}

/// One ITI-55 Initiating Gateway audit message (§3.55.5.1.1).
///
/// The source's `NetworkAccessPointID` and the `AuditSourceIdentification`
/// name the host the deployment runs on, which the client does not know: the
/// recorder adds them. No `Human Requestor` is named, since the client is
/// told of none, and no `Patient` participant, as the table requires.
#[derive(Clone)]
pub struct AuditEvent {
    /// `EventDateTime`: when the exchange ended.
    pub date_time: Timestamp,
    /// `EventOutcomeIndicator`.
    pub outcome: EventOutcome,
    /// The source's `AlternativeUserID`: this process's id in the local
    /// operating system.
    pub process_id: u32,
    /// The destination's `UserID`: the SOAP endpoint URI, without any
    /// userinfo.
    pub destination: Url,
    /// The destination's `NetworkAccessPointID` and type, when the endpoint
    /// names a host.
    pub destination_access_point: Option<NetworkAccessPoint>,
    /// The `ParticipantObjectQuery`: the request's `QueryByParameter`
    /// segment as written, which the DICOM encoding base64-encodes. It names
    /// the patient identifier.
    pub query: SecretString,
    /// The `ParticipantObjectDetail` of type [`HOME_COMMUNITY_DETAIL`]: the
    /// `homeCommunityId` the request named as the initiating community's
    /// (§3.55.4.1.2.4), when it named one.
    pub home_community: Option<HomeCommunityId>,
}

impl fmt::Debug for AuditEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuditEvent")
            .field("date_time", &self.date_time)
            .field("outcome", &self.outcome)
            .field("process_id", &self.process_id)
            .field("destination", &RedactedUrl(self.destination.as_str()))
            .field("destination_access_point", &self.destination_access_point)
            .field("query", &REDACTED)
            .field("home_community", &self.home_community)
            .finish()
    }
}

/// Where the client's audit events go: a deployment's route to its ATNA
/// Audit Record Repository (ITI TF-1 Table 27.1.3-1 groups every XCPD actor
/// with an ATNA Secure Node or Secure Application).
///
/// [`AuditRecorder::record`] is called once per exchange, on the task that
/// ran it, so it must not block: a recorder that sends over the network
/// queues the event and sends it elsewhere, and one that needs durability
/// buffers it locally.
///
/// The audit message is part of the exchange (§3.55.5.1): an event the
/// recorder cannot accept fails the discovery with
/// [`XcpdError::Audit`](super::error::XcpdError::Audit), and its answer is
/// never used.
pub trait AuditRecorder: Send + Sync {
    /// Records `event`.
    ///
    /// # Errors
    ///
    /// An [`AuditError`] when the recorder cannot accept the event.
    fn record(&self, event: AuditEvent) -> Result<(), AuditError>;
}

/// Why an audit recorder could not accept an event.
#[derive(Debug, thiserror::Error)]
#[error("the audit recorder could not accept the ITI-55 audit message")]
pub struct AuditError(#[source] pub Box<dyn std::error::Error + Send + Sync>);

/// The destination `endpoint` as the audit names it: without userinfo.
pub(super) fn destination(endpoint: &Url) -> Url {
    let mut shown = endpoint.clone();
    // NOTE: url::Url::set_username and set_password fail only for a URL that
    // cannot carry userinfo, which then has none to remove.
    let _cleared: Result<(), ()> = shown.set_username("");
    let _cleared: Result<(), ()> = shown.set_password(None);
    shown
}
