// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[hl7v2]` section: the MLLP listener, the character set of a message
//! with an empty MSH-18, the frame ceiling, the timeouts and the accepted
//! senders.
//!
//! A sender is named by MSH-3 and MSH-4, the sending application and
//! facility, and is the v2 counterpart of a Source's client id: it names the
//! identifier domain it feeds, one sender per domain (ITI TF-2 §3.8.4.1.3:
//! the Manager "shall only recognize (by configuration) a single Patient
//! Identity Source per domain"). A sender that names no domain may query and
//! not feed. `[hl7v2.namespaces]` names the HL7 namespace id of a domain, the
//! first subcomponent of an assigning authority (ITI TF-2 §3.8.4.1.3.1), so a
//! namespace-only PID-3.4 resolves and an answer's PID-3.4 is fully
//! qualified (ITI TF-2 §3.9.4.2.2.5). No specification governs the listener's
//! bounds: our own design.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::time::Duration;

use ferrobridge_hl7v2::decode::Charset;
use serde::{Deserialize, Serialize};

use crate::config::error::Error;
use crate::config::section::socket_address;

/// One `[[hl7v2.senders]]` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sender {
    /// The sending application, MSH-3.
    pub application: String,
    /// The sending facility, MSH-4.
    pub facility: String,
    /// The system of the identifier domain the sender feeds; empty for a
    /// sender that only queries.
    pub domain: String,
}

impl Sender {
    /// Returns the system of the domain the sender feeds, `None` for a
    /// sender that only queries.
    #[must_use]
    pub fn feeds(&self) -> Option<&str> {
        Some(self.domain.as_str()).filter(|domain| !domain.trim().is_empty())
    }
}

/// The `[hl7v2]` section as written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hl7v2 {
    /// Whether the MLLP listener runs.
    pub enabled: bool,
    /// The socket address the listener binds.
    pub listen: String,
    /// The HL7 table 0211 code a message with an empty MSH-18 is read in.
    pub default_charset: String,
    /// The largest message one frame may carry.
    pub frame_limit_bytes: usize,
    /// How long a connection may sit idle between frames.
    pub idle_timeout_ms: u64,
    /// How long one frame may take to arrive once it started.
    pub frame_timeout_ms: u64,
    /// The senders accepted; a message from another is acknowledged `AR`.
    pub senders: Vec<Sender>,
    /// The HL7 namespace id of each domain, by the domain's system.
    pub namespaces: BTreeMap<String, String>,
}

impl Default for Hl7v2 {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: String::from("0.0.0.0:2575"),
            default_charset: String::from("ASCII"),
            frame_limit_bytes: 1024 * 1024,
            idle_timeout_ms: 300_000,
            frame_timeout_ms: 30_000,
            senders: Vec::new(),
            namespaces: BTreeMap::new(),
        }
    }
}

/// The `[hl7v2]` section, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hl7v2Settings {
    /// The socket address the listener binds.
    pub listen: SocketAddr,
    /// The character set a message with an empty MSH-18 is read in.
    pub default_charset: Charset,
    /// The largest message one frame may carry.
    pub frame_limit: usize,
    /// How long a connection may sit idle between frames.
    pub idle_timeout: Duration,
    /// How long one frame may take to arrive once it started.
    pub frame_timeout: Duration,
    /// The senders accepted.
    pub senders: Vec<Sender>,
    /// The HL7 namespace id of each domain, by the domain's system.
    pub namespaces: BTreeMap<String, String>,
}

impl Hl7v2 {
    /// Resolves the section, `None` when the listener is off.
    ///
    /// # Errors
    /// Returns [`Error::Listen`] for a listener that does not parse,
    /// [`Error::Value`] for a charset that is no HL7 table 0211 code the
    /// decoder reads, a bound of 0, a sender without its application or
    /// facility, two senders of one domain, and a namespace that is empty or
    /// names two domains, and [`Error::DuplicateSender`] for a sender named
    /// twice.
    pub fn resolve(&self) -> Result<Option<Hl7v2Settings>, Error> {
        if !self.enabled {
            return Ok(None);
        }
        let listen = socket_address("hl7v2.listen", &self.listen)?;
        let Some(default_charset) = Charset::from_code(self.default_charset.trim()) else {
            return Err(value(
                "hl7v2.default_charset",
                "an HL7 table 0211 code: ASCII, UNICODE UTF-8 or an 8859 part",
            ));
        };
        for (key, bound) in [
            ("hl7v2.frame_limit_bytes", self.frame_limit_bytes == 0),
            ("hl7v2.idle_timeout_ms", self.idle_timeout_ms == 0),
            ("hl7v2.frame_timeout_ms", self.frame_timeout_ms == 0),
        ] {
            if bound {
                return Err(value(key, "it must be above 0"));
            }
        }
        let mut seen = BTreeSet::new();
        let mut fed = BTreeSet::new();
        for (index, sender) in self.senders.iter().enumerate() {
            if [&sender.application, &sender.facility]
                .iter()
                .any(|field| field.trim().is_empty())
            {
                return Err(Error::Value {
                    key: format!("hl7v2.senders[{index}]"),
                    rule: "a sender names its application and facility",
                });
            }
            if !seen.insert((sender.application.as_str(), sender.facility.as_str())) {
                return Err(Error::DuplicateSender { index });
            }
            if let Some(domain) = sender.feeds()
                && !fed.insert(domain)
            {
                return Err(Error::Value {
                    key: format!("hl7v2.senders[{index}].domain"),
                    rule: "one sender feeds a domain (ITI TF-2 3.8.4.1.3)",
                });
            }
        }
        let mut namespaces = BTreeSet::new();
        for (system, namespace) in &self.namespaces {
            if system.trim().is_empty() || namespace.trim().is_empty() {
                return Err(value(
                    "hl7v2.namespaces",
                    "a namespace id and the system it names, neither empty",
                ));
            }
            if !namespaces.insert(namespace.as_str()) {
                return Err(value("hl7v2.namespaces", "a namespace id names one domain"));
            }
        }
        Ok(Some(Hl7v2Settings {
            listen,
            default_charset,
            frame_limit: self.frame_limit_bytes,
            idle_timeout: Duration::from_millis(self.idle_timeout_ms),
            frame_timeout: Duration::from_millis(self.frame_timeout_ms),
            senders: self.senders.clone(),
            namespaces: self.namespaces.clone(),
        }))
    }
}

/// Returns an [`Error::Value`] under `key`.
fn value(key: &str, rule: &'static str) -> Error {
    Error::Value {
        key: key.to_owned(),
        rule,
    }
}

#[cfg(test)]
mod tests {
    use super::{Hl7v2, Sender};
    use crate::config::error::Error;

    fn sender(application: &str, facility: &str) -> Sender {
        Sender {
            application: application.to_owned(),
            facility: facility.to_owned(),
            domain: String::from("urn:oid:2.999.1"),
        }
    }

    #[test]
    fn the_listener_is_off_by_default_and_resolves_when_on() {
        assert_eq!(None, Hl7v2::default().resolve().expect("off resolves"));
        let mut west = sender("LAB", "WEST");
        west.domain = String::from("urn:oid:2.999.2");
        let on = Hl7v2 {
            enabled: true,
            senders: vec![sender("LAB", "EAST"), west],
            ..Hl7v2::default()
        };
        let resolved = on.resolve().expect("on resolves").expect("on");
        assert_eq!(2575, resolved.listen.port());
    }

    #[test]
    fn a_sender_named_twice_or_incomplete_and_a_zero_bound_are_refused() {
        let twice = Hl7v2 {
            enabled: true,
            senders: vec![sender("LAB", "EAST"), sender("LAB", "EAST")],
            ..Hl7v2::default()
        };
        assert!(matches!(
            twice.resolve(),
            Err(Error::DuplicateSender { index: 1 })
        ));
        let incomplete = Hl7v2 {
            enabled: true,
            senders: vec![sender("LAB", " ")],
            ..Hl7v2::default()
        };
        assert!(matches!(incomplete.resolve(), Err(Error::Value { .. })));
        let zero = Hl7v2 {
            enabled: true,
            frame_limit_bytes: 0,
            ..Hl7v2::default()
        };
        assert!(matches!(zero.resolve(), Err(Error::Value { .. })));
        let charset = Hl7v2 {
            enabled: true,
            default_charset: String::new(),
            ..Hl7v2::default()
        };
        assert!(matches!(charset.resolve(), Err(Error::Value { .. })));
        let unknown_charset = Hl7v2 {
            enabled: true,
            default_charset: String::from("EBCDIC"),
            ..Hl7v2::default()
        };
        assert!(matches!(
            unknown_charset.resolve(),
            Err(Error::Value { .. })
        ));
    }

    #[test]
    fn two_senders_of_one_domain_and_a_namespace_of_two_domains_are_refused() {
        let mut second = sender("LAB", "WEST");
        second.domain = String::from("urn:oid:2.999.1");
        let two = Hl7v2 {
            enabled: true,
            senders: vec![sender("LAB", "EAST"), second],
            ..Hl7v2::default()
        };
        assert!(matches!(
            two.resolve(),
            Err(Error::Value { key, .. }) if key == "hl7v2.senders[1].domain"
        ));
        let mut querying = sender("PORTAL", "EAST");
        querying.domain = String::new();
        let query_only = Hl7v2 {
            enabled: true,
            senders: vec![sender("LAB", "EAST"), querying.clone(), {
                let mut other = querying;
                other.facility = String::from("WEST");
                other
            }],
            ..Hl7v2::default()
        };
        let resolved = query_only
            .resolve()
            .expect("query-only senders resolve")
            .expect("on");
        assert_eq!(None, resolved.senders[1].feeds());
        let shared = Hl7v2 {
            enabled: true,
            namespaces: [
                (String::from("urn:oid:2.999.1"), String::from("EAST")),
                (String::from("urn:oid:2.999.2"), String::from("EAST")),
            ]
            .into_iter()
            .collect(),
            ..Hl7v2::default()
        };
        assert!(matches!(shared.resolve(), Err(Error::Value { .. })));
    }
}
