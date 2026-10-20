// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Why a PMIR exchange failed.
//!
//! No error here carries an identifier value, a request URL, or text the
//! Patient Identity Registry or a Supplier wrote (an `OperationOutcome`'s
//! `diagnostics` or `details`, a JSON parse snippet, an element path): an
//! ITI-93 message carries Patient Master Identities, and the Registry's text
//! may quote one. An entry of a message is named by its position, never by its
//! content.

use http::StatusCode;

use crate::outcome::IssueType;

/// An argument the subscriber refuses before anything is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidInput {
    /// The Registry's base URL is not an `http` or `https` URL without a
    /// query or a fragment.
    #[error(
        "the Patient Identity Registry base URL is not an http(s) URL without a query or a fragment"
    )]
    Base,
    /// The channel endpoint the feed is to be sent to is not an absolute
    /// `http` or `https` URL without a fragment.
    #[error("the channel endpoint is not an absolute http(s) URL without a fragment")]
    Endpoint,
    /// The identifier system a criteria limits the feed to is not an absolute
    /// URI.
    #[error("the identifier system of the criteria is not an absolute URI")]
    System,
    /// The id the response message is given is not a UUID in its lowercase
    /// hyphenated form.
    #[error("the response message id is not a lowercase hyphenated UUID")]
    ResponseId,
}

/// Why an ITI-94 exchange did not end as asked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SubscribeError {
    /// The Registry answered a status the interaction gives no success
    /// meaning to, with the issue types of its `OperationOutcome`, if it
    /// sent one (§2:3.94.4.1.3; FHIR R4 http.html).
    #[error("the Patient Identity Registry answered {status}")]
    Rejected {
        /// The HTTP status.
        status: StatusCode,
        /// The issue types of the `OperationOutcome`, in order.
        issues: Vec<IssueType>,
    },
    /// No answer arrived within the timeout.
    #[error("the Patient Identity Registry did not answer within the timeout")]
    Timeout,
    /// The request could not be sent or the answer could not be read.
    #[error("the Patient Identity Registry could not be reached")]
    Transport(#[source] reqwest::Error),
    /// The authorizer made no headers for the request, so nothing was sent
    /// (IUA ITI-72 §3.72.4.2).
    #[error("the request to the Patient Identity Registry could not be authenticated")]
    Unauthenticated(#[source] crate::authorizer::AuthorizerError),
    /// The `Subscription` could not be written as FHIR JSON, so nothing was
    /// sent.
    #[error("the Subscription cannot be written as FHIR JSON")]
    Unwritable(#[source] serde_json::Error),
    /// The answer does not hold to ITI-94.
    #[error("the Patient Identity Registry's answer does not hold to ITI-94")]
    Malformed(#[from] SubscriptionMalformation),
    /// The audit recorder could not accept the exchange's audit record, so
    /// its answer is not used (feature `balp`, §2:3.94.5.1).
    #[cfg(feature = "balp")]
    #[error("the ITI-94 audit record could not be recorded")]
    Audit(#[source] crate::balp::AuditError),
}

impl SubscribeError {
    /// The status the Registry answered with, when it answered.
    #[must_use]
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Rejected { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Whether the Registry answered at all, so the failure is the Registry's
    /// answer rather than the connection.
    #[must_use]
    pub fn answered(&self) -> bool {
        matches!(self, Self::Rejected { .. } | Self::Malformed(_))
    }
}

/// How an ITI-94 answer departs from the transaction (§2:3.94.4.2.2,
/// §2:3.94.4.3; FHIR R4 http.html create and read).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SubscriptionMalformation {
    /// A `201` without a `Location` header (§2:3.94.4.2.2).
    #[error("the created Subscription has no Location")]
    NoLocation,
    /// The `Location` is not a URL of a `Subscription` under the Registry's
    /// base, so nothing more is sent to it.
    #[error("the Location is not a Subscription under the Registry's base")]
    Location,
    /// The answer's media type is not FHIR JSON.
    #[error("the answer is not application/fhir+json")]
    NotFhirJson,
    /// The answer is longer than the subscriber reads.
    #[error("the answer exceeds {limit} bytes")]
    TooLarge {
        /// The limit, in bytes.
        limit: usize,
    },
    /// The answer is not JSON.
    #[error("the answer is not JSON (line {line}, column {column})")]
    NotJson {
        /// The line of the syntax error.
        line: usize,
        /// The column of the syntax error.
        column: usize,
    },
    /// The answer is not a `Subscription` resource.
    #[error("the answer is not a Subscription")]
    NotASubscription,
    /// A search answer is not a `searchset` Bundle, or holds an entry that is
    /// neither a `Subscription` nor an outcome.
    #[error("the search answer is not a searchset of Subscriptions")]
    NotASearchset,
    /// A `Subscription` the search listed has no `id`, so it cannot be read
    /// or deleted.
    #[error("a listed Subscription has no id")]
    NoId,
    /// The resource does not decode as FHIR R4.
    #[error("the resource does not decode as FHIR R4: {kind}")]
    Decode {
        /// Why it was refused.
        kind: fhir_types::codec::DecodeErrorKind,
    },
    /// The `status` is outside the R4 `subscription-status` value set.
    #[error("the Subscription status is outside the subscription-status value set")]
    Status,
}

/// Why an ITI-93 message is refused, with nothing of it applied
/// (§2:3.93.4.1.2; the PMIR Bundle, `MessageHeader`, history Bundle and merged
/// Patient profiles).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FeedError {
    /// The message's media type is not FHIR JSON, the format the
    /// subscription asked for (§2:3.93.4.1.2).
    #[error("the message is not application/fhir+json")]
    NotFhirJson,
    /// The message is not JSON.
    #[error("the message is not JSON (line {line}, column {column})")]
    NotJson {
        /// The line of the syntax error.
        line: usize,
        /// The column of the syntax error.
        column: usize,
    },
    /// The message is not a JSON object with a `resourceType`.
    #[error("the message is not a FHIR resource")]
    NotAResource,
    /// The message, or one of its entries, is a resource the profile does not
    /// put there.
    #[error("the message holds a resource the PMIR profiles do not put there")]
    UnexpectedResource,
    /// The message does not decode as FHIR R4.
    #[error("the message does not decode as FHIR R4: {kind}")]
    Decode {
        /// Why it was refused.
        kind: fhir_types::codec::DecodeErrorKind,
    },
    /// A Bundle is not of the type the profile fixes.
    #[error("a Bundle of the message is not of type {expected}")]
    BundleType {
        /// The type the profile fixes.
        expected: &'static str,
    },
    /// The message Bundle does not hold exactly two entries.
    #[error("the message Bundle holds {found} entries, not 2")]
    EntryCount {
        /// How many it holds.
        found: usize,
    },
    /// An entry of the message Bundle has no `fullUrl`.
    #[error("entry {entry} of the message Bundle has no fullUrl")]
    NoFullUrl {
        /// The entry's position.
        entry: usize,
    },
    /// The first entry is not a `MessageHeader` (§2:3.93.4.1.2.2).
    #[error("the first entry of the message is not a MessageHeader")]
    NoHeader,
    /// The `MessageHeader` has no `id`, which the response names
    /// (`MessageHeader.response.identifier`).
    #[error("the MessageHeader has no id")]
    NoMessageId,
    /// The event is not `urn:ihe:iti:pmir:2019:patient-feed`.
    #[error("the MessageHeader's event is not the PMIR patient feed")]
    Event,
    /// The `definition` names another `MessageDefinition`.
    #[error("the MessageHeader's definition is not the PMIR feed MessageDefinition")]
    Definition,
    /// The `MessageHeader` has no `destination`.
    #[error("the MessageHeader has no destination")]
    NoDestination,
    /// The `MessageHeader` does not focus exactly on the history Bundle.
    #[error("the MessageHeader does not focus on the history Bundle")]
    Focus,
    /// The second entry is not a Bundle (§2:3.93.4.1.2.3).
    #[error("the second entry of the message is not a history Bundle")]
    NoHistory,
    /// The history Bundle holds no entry.
    #[error("the history Bundle holds no entry")]
    EmptyHistory,
    /// A history entry has no `request`.
    #[error("history entry {index} has no request")]
    NoRequest {
        /// The entry's position.
        index: usize,
    },
    /// A history entry's method is not `POST`, `PUT` or `DELETE`.
    #[error("history entry {index} is not a create, an update or a delete")]
    Method {
        /// The entry's position.
        index: usize,
    },
    /// A history entry has no `response`.
    #[error("history entry {index} has no response")]
    NoResponse {
        /// The entry's position.
        index: usize,
    },
    /// A history entry's response is not a success, and only successful
    /// changes are sent (§2:3.93.4.1.2.3).
    #[error("history entry {index} reports a change that did not succeed")]
    Unsuccessful {
        /// The entry's position.
        index: usize,
    },
    /// A create or an update carries no resource.
    #[error("history entry {index} carries no resource")]
    NoResource {
        /// The entry's position.
        index: usize,
    },
    /// A history entry carries a resource that is not a `Patient`.
    #[error("history entry {index} carries a resource that is not a Patient")]
    NotAPatient {
        /// The entry's position.
        index: usize,
    },
    /// A delete's `request.url` does not name a `Patient`, or names another
    /// one than the resource it carries.
    #[error("history entry {index} does not name the Patient it changes")]
    RequestUrl {
        /// The entry's position.
        index: usize,
    },
    /// A Patient with a `replaced-by` link breaks the merged Patient profile:
    /// it is not `active` false, carries another link, is not sent as an
    /// update, or names no surviving Patient (§2:3.93.4.1.2.4).
    #[error("history entry {index} is a merge that breaks the merged Patient profile")]
    Merge {
        /// The entry's position.
        index: usize,
    },
    /// Two history entries change the same Patient, and each entry is a
    /// unique Patient (§2:3.93.4.1.2.3).
    #[error("history entry {index} changes a Patient an earlier entry changes")]
    Duplicate {
        /// The entry's position.
        index: usize,
    },
}

impl FeedError {
    /// The FHIR `issue-type` a refusal of the message is reported with.
    #[must_use]
    pub fn issue(&self) -> IssueType {
        match self {
            Self::NotFhirJson => IssueType::NotSupported,
            Self::NotJson { .. } | Self::NotAResource | Self::Decode { .. } => IssueType::Structure,
            _ => IssueType::Invalid,
        }
    }
}

/// A transport failure, with the request URL removed: it names the Registry,
/// whose base may carry a credential.
pub(super) fn transport(error: reqwest::Error) -> SubscribeError {
    let error = error.without_url();
    if error.is_timeout() {
        SubscribeError::Timeout
    } else {
        SubscribeError::Transport(error)
    }
}

/// A send that produced no answer, a transport failure read as [`transport`]
/// reads it.
pub(super) fn unsent(error: crate::authorizer::Unsent) -> SubscribeError {
    match error {
        crate::authorizer::Unsent::Unauthenticated(source) => {
            SubscribeError::Unauthenticated(source)
        }
        crate::authorizer::Unsent::Timeout => SubscribeError::Timeout,
        crate::authorizer::Unsent::Transport(source) => transport(source),
    }
}
