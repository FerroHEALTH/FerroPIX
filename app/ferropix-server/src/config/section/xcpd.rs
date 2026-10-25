// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[xcpd]` section: the XCPD Responding Gateway's switch, its
//! community, its device, the domains its answers carry and the initiating
//! communities it answers.
//!
//! The community is the `homeCommunityId` of every `RegistrationEvent`
//! (ITI TF-2 §3.55.4.2.2.4) and the device the sender of every answer
//! (§3.55.4.2.2). An identifier in an ITI-55 message is an HL7 v3 `II` whose
//! `root` is an OID (ITI TF-2 Appendix E), so each domain is a `urn:oid:`
//! system; the first domain the person holds an identifier in is the
//! `Patient.id`, the others are `OtherIDs` (§3.55.4.2.2.2). The list of
//! initiating communities is the counterpart of `[[hl7v2.senders]]`. An
//! asynchronous request names the endpoint its answer is sent to
//! (ITI TF-2 V.5), which must start with one of `reply_endpoints`, so a
//! request cannot make the gateway post to an address the operator did not
//! name; the delivery is tried a bounded number of times. The order of the
//! domains, the lists and the bounds are our own design: no specification
//! governs them.

use std::collections::BTreeSet;
use std::time::Duration;

use ihe_iti::xcpd::identifier::{HomeCommunityId, Oid};
use serde::{Deserialize, Serialize};

use crate::config::error::Error;
use crate::config::section::is_loopback_url;

/// The `[xcpd]` section as written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Xcpd {
    /// Whether `POST {base}/xcpd` answers ITI-55.
    pub enabled: bool,
    /// The community's `homeCommunityId`, `urn:oid:` and an OID.
    pub home_community_id: String,
    /// The OID of the gateway's device.
    pub device: String,
    /// The `urn:oid:` systems of the domains whose identifiers an answer
    /// carries, the `Patient.id` domain first.
    pub domains: Vec<String>,
    /// The `homeCommunityId`s of the initiating communities answered; a
    /// request from another is answered with Case 5.
    pub initiating_communities: Vec<String>,
    /// The URL prefixes an asynchronous request's `ReplyTo` may start with,
    /// each `https`, or `http` on a loopback host.
    pub reply_endpoints: Vec<String>,
    /// The most delivery attempts of one asynchronous answer.
    pub delivery_attempts: u32,
    /// The wait before the first retry of a delivery.
    pub delivery_initial_backoff_ms: u64,
    /// The longest wait between two delivery attempts.
    pub delivery_max_backoff_ms: u64,
    /// How long one delivery attempt may take.
    pub delivery_timeout_ms: u64,
}

impl Default for Xcpd {
    fn default() -> Self {
        Self {
            enabled: false,
            home_community_id: String::new(),
            device: String::new(),
            domains: Vec::new(),
            initiating_communities: Vec::new(),
            reply_endpoints: Vec::new(),
            delivery_attempts: 5,
            delivery_initial_backoff_ms: 1000,
            delivery_max_backoff_ms: 60_000,
            delivery_timeout_ms: 10_000,
        }
    }
}

/// How an asynchronous answer is delivered, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    /// The URL prefixes a `ReplyTo` may start with.
    pub endpoints: Vec<url::Url>,
    /// The most attempts.
    pub attempts: u32,
    /// The wait before the first retry.
    pub initial_backoff: Duration,
    /// The longest wait between two attempts.
    pub max_backoff: Duration,
    /// How long one attempt may take.
    pub timeout: Duration,
}

/// One answering domain, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerDomain {
    /// The domain's system, `urn:oid:` and the OID.
    pub system: String,
    /// The assigning authority, the OID.
    pub authority: Oid,
}

/// The `[xcpd]` section, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XcpdSettings {
    /// The community.
    pub community: HomeCommunityId,
    /// The gateway's device.
    pub device: Oid,
    /// The answering domains, in order.
    pub domains: Vec<AnswerDomain>,
    /// The initiating communities answered.
    pub initiating: BTreeSet<HomeCommunityId>,
    /// How an asynchronous answer is delivered.
    pub delivery: Delivery,
}

impl Xcpd {
    /// Resolves the section: `None` while it is off.
    ///
    /// # Errors
    /// Returns [`Error::Missing`] for a community, device or domain list
    /// left out, and [`Error::Value`] for a value that is no OID, a domain
    /// that is no `urn:oid:` system, and a domain or community named twice.
    pub fn resolve(&self) -> Result<Option<XcpdSettings>, Error> {
        if !self.enabled {
            return Ok(None);
        }
        let community =
            HomeCommunityId::new(urn_oid("xcpd.home_community_id", &self.home_community_id)?);
        if self.device.trim().is_empty() {
            return Err(Error::Missing {
                key: String::from("xcpd.device"),
            });
        }
        let device = Oid::new(self.device.trim()).map_err(|_invalid| Error::Value {
            key: String::from("xcpd.device"),
            rule: "an OID, dotted or as urn:oid:",
        })?;
        if self.domains.is_empty() {
            return Err(Error::Missing {
                key: String::from("xcpd.domains"),
            });
        }
        let mut domains = Vec::with_capacity(self.domains.len());
        let mut seen = BTreeSet::new();
        for (index, system) in self.domains.iter().enumerate() {
            let key = format!("xcpd.domains[{index}]");
            let authority = urn_oid(&key, system)?;
            if !seen.insert(authority.clone()) {
                return Err(Error::Value {
                    key,
                    rule: "a domain is named once",
                });
            }
            domains.push(AnswerDomain {
                system: authority.urn(),
                authority,
            });
        }
        let mut initiating = BTreeSet::new();
        for (index, text) in self.initiating_communities.iter().enumerate() {
            let key = format!("xcpd.initiating_communities[{index}]");
            if !initiating.insert(HomeCommunityId::new(urn_oid(&key, text)?)) {
                return Err(Error::Value {
                    key,
                    rule: "a community is named once",
                });
            }
        }
        Ok(Some(XcpdSettings {
            community,
            device,
            domains,
            initiating,
            delivery: self.delivery()?,
        }))
    }

    /// Resolves the delivery of asynchronous answers.
    fn delivery(&self) -> Result<Delivery, Error> {
        let mut endpoints = Vec::with_capacity(self.reply_endpoints.len());
        for (index, text) in self.reply_endpoints.iter().enumerate() {
            let key = format!("xcpd.reply_endpoints[{index}]");
            let endpoint = crate::config::section::url(&key, text.trim())?;
            let secure = endpoint.scheme() == "https"
                || (endpoint.scheme() == "http" && is_loopback_url(&endpoint));
            if !secure || endpoint.cannot_be_a_base() || !endpoint.username().is_empty() {
                return Err(Error::Value {
                    key,
                    rule: "an https URL, or http on a loopback host, with no userinfo",
                });
            }
            endpoints.push(endpoint);
        }
        for (key, zero) in [
            ("xcpd.delivery_attempts", self.delivery_attempts == 0),
            (
                "xcpd.delivery_initial_backoff_ms",
                self.delivery_initial_backoff_ms == 0,
            ),
            (
                "xcpd.delivery_max_backoff_ms",
                self.delivery_max_backoff_ms == 0,
            ),
            ("xcpd.delivery_timeout_ms", self.delivery_timeout_ms == 0),
        ] {
            if zero {
                return Err(Error::Value {
                    key: key.to_owned(),
                    rule: "it must be above 0",
                });
            }
        }
        Ok(Delivery {
            endpoints,
            attempts: self.delivery_attempts,
            initial_backoff: Duration::from_millis(self.delivery_initial_backoff_ms),
            max_backoff: Duration::from_millis(self.delivery_max_backoff_ms),
            timeout: Duration::from_millis(self.delivery_timeout_ms),
        })
    }
}

/// Reads `text` under `key` as `urn:oid:` and an OID.
fn urn_oid(key: &str, text: &str) -> Result<Oid, Error> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::Missing {
            key: key.to_owned(),
        });
    }
    let refused = || Error::Value {
        key: key.to_owned(),
        rule: "urn:oid: and an OID (RFC 3061)",
    };
    if !text.starts_with("urn:oid:") {
        return Err(refused());
    }
    Oid::new(text).map_err(|_invalid| refused())
}

#[cfg(test)]
mod tests {
    use super::Xcpd;
    use crate::config::error::Error;

    fn on() -> Xcpd {
        Xcpd {
            enabled: true,
            home_community_id: String::from("urn:oid:2.999.7"),
            device: String::from("2.999.7.1"),
            domains: vec![
                String::from("urn:oid:2.999.1.1"),
                String::from("urn:oid:2.999.1.2"),
            ],
            initiating_communities: vec![String::from("urn:oid:2.999.8")],
            reply_endpoints: vec![
                String::from("https://ig.example.org/xcpd/"),
                String::from("http://127.0.0.1:9090/"),
            ],
            ..Xcpd::default()
        }
    }

    #[test]
    fn the_gateway_is_off_by_default_and_resolves_when_on() {
        assert_eq!(None, Xcpd::default().resolve().expect("off resolves"));
        let resolved = on().resolve().expect("on resolves").expect("on");
        assert_eq!("urn:oid:2.999.7", resolved.community.to_string());
        assert_eq!("2.999.7.1", resolved.device.as_str());
        assert_eq!(
            vec!["urn:oid:2.999.1.1", "urn:oid:2.999.1.2"],
            resolved
                .domains
                .iter()
                .map(|domain| domain.system.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!("2.999.1.1", resolved.domains[0].authority.as_str());
        assert_eq!(1, resolved.initiating.len());
        assert_eq!(2, resolved.delivery.endpoints.len());
        assert_eq!(5, resolved.delivery.attempts);
    }

    fn edited(edit: impl FnOnce(&mut Xcpd)) -> Result<Option<super::XcpdSettings>, Error> {
        let mut section = on();
        edit(&mut section);
        section.resolve()
    }

    fn missing(result: &Result<Option<super::XcpdSettings>, Error>, expected: &str) {
        assert!(
            matches!(result, Err(Error::Missing { key }) if key == expected),
            "{expected}"
        );
    }

    fn refused(result: &Result<Option<super::XcpdSettings>, Error>, expected: &str) {
        assert!(
            matches!(result, Err(Error::Value { key, .. }) if key == expected),
            "{expected}"
        );
    }

    #[test]
    fn a_missing_or_malformed_value_is_refused_by_its_key() {
        missing(
            &edited(|s| s.home_community_id.clear()),
            "xcpd.home_community_id",
        );
        missing(&edited(|s| s.device.clear()), "xcpd.device");
        missing(&edited(|s| s.domains.clear()), "xcpd.domains");
        refused(
            &edited(|s| s.home_community_id = String::from("2.999.7")),
            "xcpd.home_community_id",
        );
        refused(&edited(|s| s.device = String::from("3.1")), "xcpd.device");
        refused(
            &edited(|s| s.domains[1] = String::from("https://example.org/sid")),
            "xcpd.domains[1]",
        );
        refused(
            &edited(|s| s.domains[1] = String::from("urn:oid:2.999.1.1")),
            "xcpd.domains[1]",
        );
        refused(
            &edited(|s| {
                s.initiating_communities
                    .push(String::from("urn:oid:2.999.8"));
            }),
            "xcpd.initiating_communities[1]",
        );
        refused(
            &edited(|s| s.reply_endpoints[1] = String::from("http://ig.example.org/")),
            "xcpd.reply_endpoints[1]",
        );
        refused(
            &edited(|s| s.reply_endpoints[0] = String::from("https://user@ig.example.org/")),
            "xcpd.reply_endpoints[0]",
        );
        refused(
            &edited(|s| s.delivery_attempts = 0),
            "xcpd.delivery_attempts",
        );
    }
}
