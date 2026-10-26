// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-94 as the Patient Identity Registry receives it: a `Subscription`
//! read against the PMIR Subscription profile, with its criteria
//! (§2:3.94.4.1.2.1, §2:3.94.4.1.2.1.1).

use std::fmt;

use fhir_types::codec::{Json, Path, Value};
use fhir_types::r4::operation_outcome::{OperationOutcome, OperationOutcomeIssue};
use fhir_types::r4::patient::Patient;
use fhir_types::r4::subscription::Subscription;
use http::StatusCode;
use secrecy::{ExposeSecret as _, SecretString};
use url::Url;

use crate::outcome::IssueType;
use crate::server::answer::Answer;
use crate::server::format::{BodyError, Format};
use crate::server::token;
use ihe_atna::redact::{REDACTED, RedactedUrl};

/// Which Patients a subscription reports (§2:3.94.4.1.2.1.1).
///
/// `Debug` shows the kind of criteria and redacts an identifier's value,
/// which may name a patient.
#[derive(Clone)]
#[non_exhaustive]
pub enum Criteria {
    /// `Patient`: every Patient update.
    All,
    /// `Patient?_id=X`: one Patient, by its resource id.
    Id(String),
    /// `Patient?organization=X`: the Patients of one Organization, by its
    /// reference or id.
    Organization(String),
    /// `Patient?identifier=X`: the Patients carrying an identifier, the FHIR
    /// token `[system]|[value]`, `|[value]`, `[system]|` or `[value]` (FHIR R4
    /// search.html#token).
    Identifier {
        /// The system the token names: `Some("")` for `|value`, which asks
        /// for an identifier with no system, and `None` for a bare value.
        system: Option<String>,
        /// The value the token names, `None` for `system|`.
        value: Option<SecretString>,
    },
}

impl fmt::Debug for Criteria {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("All"),
            Self::Id(_) => f.debug_tuple("Id").field(&REDACTED).finish(),
            Self::Organization(organization) => {
                f.debug_tuple("Organization").field(organization).finish()
            }
            Self::Identifier { system, value } => f
                .debug_struct("Identifier")
                .field("system", system)
                .field("value", &value.as_ref().map(|_| REDACTED))
                .finish(),
        }
    }
}

impl Criteria {
    /// Reads the criteria text of a `Subscription`, one of the four the
    /// Registry supports (§2:3.94.4.1.2.1.1), its value percent-decoded and
    /// an identifier read as a FHIR token, its escapes undone.
    ///
    /// # Errors
    /// [`SubscriptionError::Criteria`] for any other criteria.
    pub fn parse(text: &str) -> Result<Self, SubscriptionError> {
        let Some(rest) = text.strip_prefix("Patient") else {
            return Err(SubscriptionError::Criteria);
        };
        if rest.is_empty() {
            return Ok(Self::All);
        }
        let query = rest.strip_prefix('?').ok_or(SubscriptionError::Criteria)?;
        let [(name, value)] = token::url_query(query)
            .try_into()
            .map_err(|_more| SubscriptionError::Criteria)?;
        if value.is_empty() {
            return Err(SubscriptionError::Criteria);
        }
        match name.as_str() {
            "_id" => Ok(Self::Id(value)),
            "organization" => Ok(Self::Organization(value)),
            "identifier" => {
                let token = token::token(&value);
                Ok(Self::Identifier {
                    system: token.system,
                    value: (!token.code.expose_secret().is_empty()).then_some(token.code),
                })
            }
            _ => Err(SubscriptionError::Criteria),
        }
    }

    /// Whether `patient` falls under the criteria, as a search with them
    /// would find it (FHIR R4 search.html: `_id`, the `organization`
    /// reference parameter of `Patient`, and the `identifier` token).
    #[must_use]
    pub fn matches(&self, patient: &Patient) -> bool {
        match self {
            Self::All => true,
            Self::Id(id) => patient.id.as_deref() == Some(id.as_str()),
            Self::Organization(organization) => patient
                .managing_organization
                .as_ref()
                .and_then(|reference| reference.reference.as_ref())
                .and_then(|reference| reference.value.as_deref())
                .is_some_and(|reference| {
                    reference == organization
                        || reference
                            .strip_suffix(organization.as_str())
                            .is_some_and(|base| base.ends_with("Organization/"))
                }),
            Self::Identifier { system, value } => patient.identifier.iter().any(|identifier| {
                let held_system = identifier
                    .system
                    .as_ref()
                    .and_then(|system| system.value.as_deref());
                let held_value = identifier
                    .value
                    .as_ref()
                    .and_then(|value| value.value.as_deref());
                let system_ok = match system.as_deref() {
                    None => true,
                    Some("") => held_system.is_none(),
                    Some(system) => held_system == Some(system),
                };
                let value_ok = value
                    .as_ref()
                    .is_none_or(|value| held_value == Some(value.expose_secret()));
                system_ok && value_ok
            }),
        }
    }
}

/// A `Subscription` an ITI-94 create or update carries, read against the
/// PMIR Subscription profile.
///
/// `Debug` shows the status and the criteria's kind, and redacts the
/// endpoint's userinfo and query.
#[derive(Clone)]
pub struct RequestedSubscription {
    criteria: Criteria,
    endpoint: Url,
    payload: String,
    status: String,
    resource: Box<Subscription>,
}

impl fmt::Debug for RequestedSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestedSubscription")
            .field("criteria", &self.criteria)
            .field("endpoint", &RedactedUrl(self.endpoint.as_str()))
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl RequestedSubscription {
    /// Reads the `Subscription` `body`, in `format`.
    ///
    /// It holds to the PMIR Subscription profile: a `message` channel with
    /// an absolute `http` or `https` endpoint and a payload media type, every
    /// contact with its system and value, and criteria one of the four
    /// (§2:3.94.4.1.2.1, §2:3.94.4.1.2.1.1).
    ///
    /// # Errors
    /// A [`SubscriptionError`] for a body that is no `Subscription` in
    /// `format`, or one that departs from the profile; its
    /// [`refusal`](SubscriptionError::refusal) is the answer to send.
    pub fn read(format: Format, body: &[u8]) -> Result<Self, SubscriptionError> {
        let object = format.decode(body).map_err(SubscriptionError::Body)?;
        if object.get("resourceType").and_then(Value::as_str) != Some("Subscription") {
            return Err(SubscriptionError::NotASubscription);
        }
        let resource = Subscription::from_json(&object, &mut Path::root("Subscription"))
            .map_err(|error| SubscriptionError::Body(BodyError::decode(error)))?;
        let criteria = Criteria::parse(resource.criteria.value.as_deref().unwrap_or_default())?;
        if resource.channel.r#type.value.as_deref() != Some("message") {
            return Err(SubscriptionError::Channel);
        }
        let endpoint = resource
            .channel
            .endpoint
            .as_ref()
            .and_then(|endpoint| endpoint.value.as_deref())
            .and_then(|endpoint| Url::parse(endpoint).ok())
            .filter(|endpoint| matches!(endpoint.scheme(), "https" | "http"))
            .ok_or(SubscriptionError::Endpoint)?;
        let payload = resource
            .channel
            .payload
            .as_ref()
            .and_then(|payload| payload.value.clone())
            .filter(|payload| !payload.is_empty())
            .ok_or(SubscriptionError::Payload)?;
        let contacts = resource.contact.iter().all(|contact| {
            contact
                .system
                .as_ref()
                .is_some_and(|system| system.value.is_some())
                && contact
                    .value
                    .as_ref()
                    .is_some_and(|value| value.value.is_some())
        });
        if !contacts {
            return Err(SubscriptionError::Contact);
        }
        let status = resource.status.value.clone().unwrap_or_default();
        Ok(Self {
            criteria,
            endpoint,
            payload,
            status,
            resource: Box::new(resource),
        })
    }

    /// The criteria.
    #[must_use]
    pub fn criteria(&self) -> &Criteria {
        &self.criteria
    }

    /// The channel endpoint every ITI-93 message goes to.
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// The media type the subscriber asked the feed in
    /// (`channel.payload`).
    #[must_use]
    pub fn payload(&self) -> &str {
        &self.payload
    }

    /// The status the request states: `requested` for a create (the PMIR
    /// Subscription request profile), `off` or `requested` for an enable or
    /// disable (§2:3.94.4.4).
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// The `Subscription` as read, which the Registry stores and answers a
    /// read with (§2:3.94.4.3).
    #[must_use]
    pub fn resource(&self) -> &Subscription {
        &self.resource
    }
}

/// Why a `Subscription` is refused.
///
/// No variant carries text from the body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SubscriptionError {
    /// The body is no FHIR R4 resource in the format it was sent in.
    #[error(transparent)]
    Body(BodyError),
    /// The body is a resource other than a `Subscription`.
    #[error("the body is not a Subscription")]
    NotASubscription,
    /// The criteria are none of `Patient`, `Patient?_id=`,
    /// `Patient?organization=` and `Patient?identifier=`
    /// (§2:3.94.4.1.2.1.1).
    #[error("the criteria are none of the four PMIR supports")]
    Criteria,
    /// The channel is not of type `message` (the PMIR Subscription profile).
    #[error("the channel is not of type message")]
    Channel,
    /// The channel names no absolute `http` or `https` endpoint.
    #[error("the channel names no absolute http or https endpoint")]
    Endpoint,
    /// The channel names no payload media type.
    #[error("the channel names no payload media type")]
    Payload,
    /// A contact lacks its system or its value.
    #[error("a contact lacks its system or its value")]
    Contact,
}

impl SubscriptionError {
    /// The answer that refuses the `Subscription`: `400` with an
    /// `OperationOutcome` (FHIR R4 http.html#create).
    #[must_use]
    pub fn refusal(&self) -> Answer<OperationOutcome> {
        let code = match self {
            Self::Body(_) | Self::NotASubscription => IssueType::Structure,
            Self::Criteria => IssueType::NotSupported,
            _ => IssueType::Invalid,
        };
        Answer::new(
            StatusCode::BAD_REQUEST,
            OperationOutcome {
                issue: vec![OperationOutcomeIssue {
                    severity: "error".into(),
                    code: code.to_string().into(),
                    diagnostics: Some(self.to_string().into()),
                    ..OperationOutcomeIssue::default()
                }],
                ..OperationOutcome::default()
            },
        )
    }
}
