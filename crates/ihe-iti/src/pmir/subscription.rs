// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `Subscription` a Patient Identity Subscriber creates with ITI-94, and
//! what the Registry says of it (§2:3.94.4.1.2, §2:3.94.4.3).

use std::fmt;

use fhir_types::codec::{Json, Object, Path, Value};
use fhir_types::r4::bundle::Bundle;
use fhir_types::r4::resource::Resource;
use fhir_types::r4::subscription::{Subscription, SubscriptionChannel};
use url::Url;

use crate::outcome;
use crate::redact::RedactedUrl;
use crate::search::escape;

use super::error::{InvalidInput, SubscriptionMalformation};

/// The media type the feed is asked in and the subscriber reads (ITI TF-2
/// Appendix Z.6).
pub(super) const FHIR_JSON: &str = "application/fhir+json";

/// The reason a subscription states (`Subscription.reason`, 1..1 in R4).
const REASON: &str = "Patient Master Identity changes for a federation gateway's resolution state";

/// Which Patients the feed reports (§2:3.94.4.1.2.1.1).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Criteria {
    /// `Patient`: every Patient update.
    AllPatients,
    /// `Patient?identifier=<system>|`: the Patients with an identifier in one
    /// assigning authority, the token limited by system alone (FHIR R4
    /// search, token).
    IdentifierSystem(String),
}

impl Criteria {
    /// The Patients with an identifier issued by `system`.
    ///
    /// # Errors
    /// [`InvalidInput::System`] when `system` is not an absolute URI.
    pub fn identifier_system(system: &str) -> Result<Self, InvalidInput> {
        Url::parse(system).map_err(|_unparsable| InvalidInput::System)?;
        Ok(Self::IdentifierSystem(system.to_owned()))
    }

    /// The criteria as the `Subscription` writes it.
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::AllPatients => String::from("Patient"),
            Self::IdentifierSystem(system) => format!("Patient?identifier={}|", escape(system)),
        }
    }
}

/// An ITI-94 Subscribe to Patient Updates request (§2:3.94.4.1.2.1, the PMIR
/// Subscription request profile).
///
/// The channel is `message` with a FHIR JSON payload, so the Registry sends
/// each ITI-93 message to `endpoint`. It carries no `channel.header`: no
/// credential for the feed travels in the subscription (§2:3.94.5). `Debug`
/// shows the endpoint with its userinfo and its query redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct SubscriptionRequest {
    criteria: Criteria,
    endpoint: Url,
}

impl SubscriptionRequest {
    /// A subscription for the Patients `criteria` names, whose feed is sent to
    /// `endpoint`.
    ///
    /// # Errors
    /// [`InvalidInput::Endpoint`] when `endpoint` is not an `http` or `https`
    /// URL without a fragment.
    pub fn new(criteria: Criteria, endpoint: Url) -> Result<Self, InvalidInput> {
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.cannot_be_a_base()
            || endpoint.fragment().is_some()
        {
            return Err(InvalidInput::Endpoint);
        }
        Ok(Self { criteria, endpoint })
    }

    /// Which Patients the feed reports.
    #[must_use]
    pub fn criteria(&self) -> &Criteria {
        &self.criteria
    }

    /// Where the feed is sent.
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// The `Subscription` resource the request creates: `status` requested,
    /// the criteria, and a `message` channel to the endpoint with a FHIR JSON
    /// payload.
    #[must_use]
    pub fn resource(&self) -> Subscription {
        Subscription {
            status: "requested".into(),
            reason: REASON.into(),
            criteria: self.criteria.text().into(),
            channel: SubscriptionChannel {
                r#type: "message".into(),
                endpoint: Some(self.endpoint.as_str().into()),
                payload: Some(FHIR_JSON.into()),
                ..SubscriptionChannel::default()
            },
            ..Subscription::default()
        }
    }
}

impl fmt::Debug for SubscriptionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubscriptionRequest")
            .field("criteria", &self.criteria)
            .field("endpoint", &RedactedUrl(self.endpoint.as_str()))
            .finish()
    }
}

/// A subscription the Registry created: where it can be read, updated and
/// deleted (§2:3.94.4.2.2).
///
/// `Debug` shows the location with its userinfo and its query redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct Subscribed {
    location: Url,
}

impl Subscribed {
    /// The `[base]/Subscription/[id]` URL of the subscription, without a
    /// version.
    #[must_use]
    pub fn location(&self) -> &Url {
        &self.location
    }

    /// The subscription at the `Location` the Registry answered, resolved
    /// against `endpoint`, the `[base]/Subscription` it was created at.
    ///
    /// Only a `Subscription` under that same base is accepted, so a later
    /// read or delete, which carries the Registry's credentials, never goes
    /// elsewhere.
    pub(super) fn at(endpoint: &Url, location: &str) -> Result<Self, SubscriptionMalformation> {
        let resolved = endpoint
            .join(location)
            .map_err(|_unparsable| SubscriptionMalformation::Location)?;
        let prefix = format!("{}/", endpoint.as_str().trim_end_matches('/'));
        let tail = resolved
            .as_str()
            .strip_prefix(prefix.as_str())
            .ok_or(SubscriptionMalformation::Location)?;
        let id = match tail.split('/').collect::<Vec<_>>().as_slice() {
            [id] | [id, "_history", _] if !id.is_empty() => (*id).to_owned(),
            _ => return Err(SubscriptionMalformation::Location),
        };
        let location = Url::parse(&format!("{prefix}{id}"))
            .map_err(|_unparsable| SubscriptionMalformation::Location)?;
        Ok(Self { location })
    }
}

impl fmt::Debug for Subscribed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscribed")
            .field("location", &RedactedUrl(self.location.as_str()))
            .finish()
    }
}

/// The status of a subscription (FHIR R4 `subscription-status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubscriptionStatus {
    /// `requested`: not yet active.
    Requested,
    /// `active`: the feed is sent.
    Active,
    /// `error`: the Registry could not send the feed (§2:3.94.4.1.3).
    Error,
    /// `off`: the subscription is disabled (§2:3.94.4.4).
    Off,
}

impl SubscriptionStatus {
    /// The status `code` names, or `None` outside the value set.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "requested" => Some(Self::Requested),
            "active" => Some(Self::Active),
            "error" => Some(Self::Error),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    /// The status as the value set spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Active => "active",
            Self::Error => "error",
            Self::Off => "off",
        }
    }
}

/// The status a read of the subscription answered with `media` and `body`
/// holds.
pub(super) fn status(
    media: Option<&str>,
    body: &[u8],
) -> Result<SubscriptionStatus, SubscriptionMalformation> {
    if !outcome::fhir_json(media) {
        return Err(SubscriptionMalformation::NotFhirJson);
    }
    let value: Value =
        serde_json::from_slice(body).map_err(|error| SubscriptionMalformation::NotJson {
            line: error.line(),
            column: error.column(),
        })?;
    let object: &Object = value
        .as_object()
        .ok_or(SubscriptionMalformation::NotASubscription)?;
    if object.get("resourceType").and_then(Value::as_str) != Some("Subscription") {
        return Err(SubscriptionMalformation::NotASubscription);
    }
    let subscription = Subscription::from_json(object, &mut Path::root("Subscription"))
        .map_err(|error| SubscriptionMalformation::Decode { kind: error.kind })?;
    subscription
        .status
        .value
        .as_deref()
        .and_then(SubscriptionStatus::from_code)
        .ok_or(SubscriptionMalformation::Status)
}

/// A subscription a search found: where it is, and its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// Where the subscription can be read, updated and deleted.
    pub subscribed: Subscribed,
    /// Its status.
    pub status: SubscriptionStatus,
}

/// What a search for a request's subscriptions found (FHIR R4 search, the
/// `Subscription` search parameter `url`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Search {
    /// The subscriptions whose criteria, `message` channel and endpoint are
    /// the request's, in the answer's order; empty when the Registry holds
    /// none.
    Found(Vec<Listed>),
    /// The Registry does not search `Subscription` by `url`: it answered `400`
    /// or `404` (FHIR R4 search, Handling Errors).
    Unsupported,
}

/// The subscriptions of `request` a search answer with `media` and `body`
/// lists, each located under `endpoint`, the `[base]/Subscription` it was
/// asked at.
///
/// A Registry that ignores the `url` parameter lists other subscriptions as
/// well, so each is matched on its criteria, its channel type and its
/// endpoint; only the first page is read.
pub(super) fn listed(
    media: Option<&str>,
    body: &[u8],
    request: &SubscriptionRequest,
    endpoint: &Url,
) -> Result<Vec<Listed>, SubscriptionMalformation> {
    if !outcome::fhir_json(media) {
        return Err(SubscriptionMalformation::NotFhirJson);
    }
    let value: Value =
        serde_json::from_slice(body).map_err(|error| SubscriptionMalformation::NotJson {
            line: error.line(),
            column: error.column(),
        })?;
    let object: &Object = value
        .as_object()
        .ok_or(SubscriptionMalformation::NotASearchset)?;
    if object.get("resourceType").and_then(Value::as_str) != Some("Bundle") {
        return Err(SubscriptionMalformation::NotASearchset);
    }
    let bundle = Bundle::from_json(object, &mut Path::root("Bundle"))
        .map_err(|error| SubscriptionMalformation::Decode { kind: error.kind })?;
    if bundle.r#type.value.as_deref() != Some("searchset") {
        return Err(SubscriptionMalformation::NotASearchset);
    }
    let criteria = request.criteria.text();
    let mut found = Vec::new();
    for entry in bundle.entry {
        let subscription = match entry.resource {
            Some(Resource::Subscription(subscription)) => subscription,
            // NOTE: FHIR R4 search, search.mode `outcome`: an OperationOutcome in a
            // searchset tells about the search and lists no subscription.
            Some(Resource::OperationOutcome(_)) => continue,
            _ => return Err(SubscriptionMalformation::NotASearchset),
        };
        let channel = &subscription.channel;
        let ours = subscription.criteria.value.as_deref() == Some(criteria.as_str())
            && channel.r#type.value.as_deref() == Some("message")
            && channel
                .endpoint
                .as_ref()
                .and_then(|url| url.value.as_deref())
                == Some(request.endpoint.as_str());
        if !ours {
            continue;
        }
        let id = subscription
            .id
            .as_deref()
            .filter(|id| !id.is_empty())
            .ok_or(SubscriptionMalformation::NoId)?;
        let status = subscription
            .status
            .value
            .as_deref()
            .and_then(SubscriptionStatus::from_code)
            .ok_or(SubscriptionMalformation::Status)?;
        found.push(Listed {
            subscribed: Subscribed::at(endpoint, &format!("Subscription/{id}"))?,
            status,
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::{Criteria, Subscribed};
    use crate::pmir::error::SubscriptionMalformation;
    use url::Url;

    #[test]
    fn a_criteria_limited_by_system_escapes_the_separators() {
        let criteria = Criteria::identifier_system("urn:oid:2.999.1").expect("an absolute URI");
        assert_eq!("Patient?identifier=urn:oid:2.999.1|", criteria.text());
        assert_eq!("Patient", Criteria::AllPatients.text());
        assert!(Criteria::identifier_system("not a uri").is_err());
    }

    #[test]
    fn a_location_is_kept_only_under_the_base_and_without_its_version() {
        let endpoint = Url::parse("https://pmir.example.org/fhir/Subscription").expect("a URL");
        for location in [
            "Subscription/s1",
            "https://pmir.example.org/fhir/Subscription/s1",
            "https://pmir.example.org/fhir/Subscription/s1/_history/2",
        ] {
            let subscribed = Subscribed::at(&endpoint, location).expect("under the base");
            assert_eq!(
                "https://pmir.example.org/fhir/Subscription/s1",
                subscribed.location().as_str(),
                "{location}"
            );
        }
        for location in [
            "https://elsewhere.example.org/fhir/Subscription/s1",
            "Patient/s1",
            "Subscription/",
            "Subscription/s1/extra",
        ] {
            assert_eq!(
                Err(SubscriptionMalformation::Location),
                Subscribed::at(&endpoint, location),
                "{location}"
            );
        }
    }
}
