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
//!
//! `[[hl7v2.consumers]]` is the Manager's list of the Patient Identifier
//! Cross-reference Consumers ITI-10 notifies, each with the patient
//! identifier domains of interest to it, an empty list naming every domain
//! (ITI TF-2 §3.10.4.1.2: the configuration "shall include identification of
//! the identity consumer systems interested in receiving notifications and,
//! for each of those systems, a list of the patient identifier domains of
//! interest"). `application` and `facility` are the Manager's own MSH-3 and
//! MSH-4, and `[hl7v2.notification]` bounds the retries. No specification
//! governs the address, the timeouts or the retry bounds: our own design.

pub mod authority;

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::time::Duration;

use ferrobridge_hl7v2::decode::Charset;
use ferrohealth_config::removed::{Removals, Removed};
use ferrohealth_config::units::{ByteSize, Duration as Span};
use serde::{Deserialize, Serialize};

use crate::config::error::Error;
use crate::config::retired;
use crate::config::section::socket_address;
use crate::config::section::terminology::TerminologySettings;

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

/// One `[[hl7v2.consumers]]` entry: a Patient Identifier Cross-reference
/// Consumer ITI-10 notifies.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Consumer {
    /// The name the outbox, the log and the dependency report know it by.
    pub name: String,
    /// The `host:port` its MLLP listener is reached at.
    pub address: String,
    /// The receiving application, MSH-5.
    pub application: String,
    /// The receiving facility, MSH-6.
    pub facility: String,
    /// The systems of the identifier domains of interest to it; empty for
    /// every domain.
    pub domains: Vec<String>,
    /// How long opening the connection may take.
    pub connect_timeout: Span,
    /// How long the acknowledgment may take once connected.
    pub reply_timeout: Span,
    /// Retired in v0.0.23 for `connect_timeout`.
    #[serde(skip_serializing)]
    pub connect_timeout_ms: Removed,
    /// Retired in v0.0.23 for `reply_timeout`.
    #[serde(skip_serializing)]
    pub reply_timeout_ms: Removed,
}

impl Default for Consumer {
    fn default() -> Self {
        Self {
            name: String::new(),
            address: String::new(),
            application: String::new(),
            facility: String::new(),
            domains: Vec::new(),
            connect_timeout: Span::from_secs(5),
            reply_timeout: Span::from_secs(30),
            connect_timeout_ms: Removed::default(),
            reply_timeout_ms: Removed::default(),
        }
    }
}

impl Consumer {
    /// Returns whether a person holding an identifier of the domain `system`
    /// is of interest to the consumer.
    #[must_use]
    pub fn wants(&self, system: &str) -> bool {
        self.domains.is_empty() || self.domains.iter().any(|domain| domain == system)
    }
}

/// The `[hl7v2.notification]` table: the retry bounds of the ITI-10 sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Notification {
    /// The most attempts one notification is given.
    pub max_attempts: u32,
    /// The wait before the first retry.
    pub initial_backoff: Span,
    /// The longest wait between two attempts.
    pub max_backoff: Span,
    /// Retired in v0.0.23 for `initial_backoff`.
    #[serde(skip_serializing)]
    pub initial_backoff_ms: Removed,
    /// Retired in v0.0.23 for `max_backoff`.
    #[serde(skip_serializing)]
    pub max_backoff_ms: Removed,
}

impl Default for Notification {
    fn default() -> Self {
        Self {
            max_attempts: 10,
            initial_backoff: Span::from_secs(1),
            max_backoff: Span::from_mins(5),
            initial_backoff_ms: Removed::default(),
            max_backoff_ms: Removed::default(),
        }
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
    pub frame_limit: ByteSize,
    /// How long a connection may sit idle between frames.
    pub idle_timeout: Span,
    /// How long one frame may take to arrive once it started.
    pub frame_timeout: Span,
    /// The senders accepted; a message from another is acknowledged `AR`.
    pub senders: Vec<Sender>,
    /// The HL7 namespace id of each domain, by the domain's system.
    pub namespaces: BTreeMap<String, String>,
    /// The Manager's sending application, MSH-3 of an ITI-10 notification.
    pub application: String,
    /// The Manager's sending facility, MSH-4 of an ITI-10 notification.
    pub facility: String,
    /// The consumers ITI-10 notifies.
    pub consumers: Vec<Consumer>,
    /// The retry bounds of the notifications.
    pub notification: Notification,
    /// Retired in v0.0.23 for `frame_limit`.
    #[serde(skip_serializing)]
    pub frame_limit_bytes: Removed,
    /// Retired in v0.0.23 for `idle_timeout`.
    #[serde(skip_serializing)]
    pub idle_timeout_ms: Removed,
    /// Retired in v0.0.23 for `frame_timeout`.
    #[serde(skip_serializing)]
    pub frame_timeout_ms: Removed,
}

impl Default for Hl7v2 {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: String::from("0.0.0.0:2575"),
            default_charset: String::from("ASCII"),
            frame_limit: ByteSize::from_mib(1),
            idle_timeout: Span::from_mins(5),
            frame_timeout: Span::from_secs(30),
            senders: Vec::new(),
            namespaces: BTreeMap::new(),
            application: String::from("FERROPIX"),
            facility: String::from("FERROPIX"),
            consumers: Vec::new(),
            notification: Notification::default(),
            frame_limit_bytes: Removed::default(),
            idle_timeout_ms: Removed::default(),
            frame_timeout_ms: Removed::default(),
        }
    }
}

/// The `[hl7v2]` section, resolved.
#[derive(Debug, Clone)]
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
    /// The Manager's sending application and facility, MSH-3 and MSH-4.
    pub manager: (String, String),
    /// The consumers ITI-10 notifies.
    pub consumers: Vec<Consumer>,
    /// The retry bounds of the notifications.
    pub notification: Notification,
    /// The terminology server every v2 table value is translated on
    /// (`[terminology]`).
    pub terminology: ferrobridge_term::client::Client,
}

impl Hl7v2 {
    /// Gathers the retired keys the sources set.
    pub fn removals(&self, found: &mut Removals) {
        found.check(self.frame_limit_bytes, &retired::hl7v2::FRAME_LIMIT_BYTES);
        found.check(self.idle_timeout_ms, &retired::hl7v2::IDLE_TIMEOUT_MS);
        found.check(self.frame_timeout_ms, &retired::hl7v2::FRAME_TIMEOUT_MS);
        for (index, consumer) in self.consumers.iter().enumerate() {
            let table = format!("hl7v2.consumers[{index}]");
            found.check_under(
                consumer.connect_timeout_ms,
                &retired::hl7v2::CONNECT_TIMEOUT_MS,
                &table,
            );
            found.check_under(
                consumer.reply_timeout_ms,
                &retired::hl7v2::REPLY_TIMEOUT_MS,
                &table,
            );
        }
        found.check(
            self.notification.initial_backoff_ms,
            &retired::hl7v2::INITIAL_BACKOFF_MS,
        );
        found.check(
            self.notification.max_backoff_ms,
            &retired::hl7v2::MAX_BACKOFF_MS,
        );
    }

    /// Resolves the section, `None` when the listener is off.
    ///
    /// # Errors
    /// Returns [`Error::Listen`] for a listener that does not parse,
    /// [`Error::Value`] for a charset that is no HL7 table 0211 code the
    /// decoder reads, a bound of 0, a sender without its application or
    /// facility, two senders of one domain, a namespace that is empty or
    /// names two domains, an empty Manager application or facility, a
    /// consumer without its name, its `host:port`, its application or
    /// facility, with an empty domain or a zero timeout, two consumers of one
    /// name, retry bounds out of order, and no `terminology` while the
    /// face is on, and [`Error::DuplicateSender`] for a sender named twice.
    pub fn resolve(
        &self,
        terminology: Option<&TerminologySettings>,
    ) -> Result<Option<Hl7v2Settings>, Error> {
        if !self.enabled {
            return Ok(None);
        }
        // NOTE: no specification governs this: our own design; the face translates every
        // v2 table value on the terminology server and holds no copy of a table.
        let Some(terminology) = terminology else {
            return Err(value(
                "terminology.base_url",
                "set while [hl7v2] is on: the HL7 v2 face translates every table value on it",
            ));
        };
        let listen = socket_address("hl7v2.listen", &self.listen)?;
        let Some(default_charset) = Charset::from_code(self.default_charset.trim()) else {
            return Err(value(
                "hl7v2.default_charset",
                "an HL7 table 0211 code: ASCII, UNICODE UTF-8 or an 8859 part",
            ));
        };
        for (key, bound) in [
            ("hl7v2.frame_limit", self.frame_limit.is_zero()),
            ("hl7v2.idle_timeout", self.idle_timeout.is_zero()),
            ("hl7v2.frame_timeout", self.frame_timeout.is_zero()),
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
        self.check_consumers()?;
        Ok(Some(Hl7v2Settings {
            listen,
            default_charset,
            frame_limit: usize::try_from(self.frame_limit)
                .map_err(|_overflow| value("hl7v2.frame_limit", "a size this platform can hold"))?,
            idle_timeout: self.idle_timeout.get(),
            frame_timeout: self.frame_timeout.get(),
            senders: self.senders.clone(),
            namespaces: self.namespaces.clone(),
            manager: (self.application.clone(), self.facility.clone()),
            consumers: self.consumers.clone(),
            notification: self.notification,
            terminology: terminology.client.clone(),
        }))
    }

    /// Checks the Manager's identity, the consumers and the retry bounds.
    fn check_consumers(&self) -> Result<(), Error> {
        if self.application.trim().is_empty() {
            return Err(value(
                "hl7v2.application",
                "the Manager's MSH-3 is not empty",
            ));
        }
        if self.facility.trim().is_empty() {
            return Err(value("hl7v2.facility", "the Manager's MSH-4 is not empty"));
        }
        let mut names = BTreeSet::new();
        for (index, consumer) in self.consumers.iter().enumerate() {
            let key = |field: &str| format!("hl7v2.consumers[{index}].{field}");
            let refuse = |field: &str, rule: &'static str| Error::Value {
                key: key(field),
                rule,
            };
            for (field, text) in [
                ("name", &consumer.name),
                ("application", &consumer.application),
                ("facility", &consumer.facility),
            ] {
                if text.trim().is_empty() {
                    return Err(refuse(field, "it is not empty"));
                }
            }
            if !names.insert(consumer.name.as_str()) {
                return Err(refuse("name", "a consumer's name is not used twice"));
            }
            if !has_port(&consumer.address) {
                return Err(refuse("address", "a host and a port, `host:port`"));
            }
            if consumer
                .domains
                .iter()
                .any(|domain| domain.trim().is_empty())
            {
                return Err(refuse("domains", "each domain is a system, not empty"));
            }
            if consumer.connect_timeout.is_zero() {
                return Err(refuse("connect_timeout", "it must be above 0"));
            }
            if consumer.reply_timeout.is_zero() {
                return Err(refuse("reply_timeout", "it must be above 0"));
            }
        }
        let bounds = &self.notification;
        if bounds.max_attempts == 0 {
            return Err(value(
                "hl7v2.notification.max_attempts",
                "it must be above 0",
            ));
        }
        if bounds.initial_backoff.is_zero() {
            return Err(value(
                "hl7v2.notification.initial_backoff",
                "it must be above 0",
            ));
        }
        if bounds.initial_backoff > bounds.max_backoff {
            return Err(value(
                "hl7v2.notification.max_backoff",
                "it must be at least hl7v2.notification.initial_backoff",
            ));
        }
        Ok(())
    }
}

/// Returns whether `address` names a host and a port above 0.
fn has_port(address: &str) -> bool {
    address.rsplit_once(':').is_some_and(|(host, port)| {
        !host.trim().is_empty() && port.parse::<u16>().is_ok_and(|port| port > 0)
    })
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
    use super::{Consumer, Hl7v2, Sender};
    use crate::config::Profile;
    use crate::config::error::Error;
    use crate::config::section::terminology::{Terminology, TerminologySettings};

    /// A terminology server the face translates on; none is reached.
    fn tx() -> TerminologySettings {
        Terminology {
            base_url: String::from("https://tx.example.org/r4"),
            ..Terminology::default()
        }
        .resolve(Profile::Production)
        .expect("the section resolves")
        .expect("a terminology server")
    }

    fn sender(application: &str, facility: &str) -> Sender {
        Sender {
            application: application.to_owned(),
            facility: facility.to_owned(),
            domain: String::from("urn:oid:2.999.1"),
        }
    }

    #[test]
    fn the_listener_is_off_by_default_and_resolves_when_on() {
        assert!(
            Hl7v2::default()
                .resolve(None)
                .expect("off resolves")
                .is_none()
        );
        let mut west = sender("LAB", "WEST");
        west.domain = String::from("urn:oid:2.999.2");
        let on = Hl7v2 {
            enabled: true,
            senders: vec![sender("LAB", "EAST"), west],
            ..Hl7v2::default()
        };
        let resolved = on.resolve(Some(&tx())).expect("on resolves").expect("on");
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
            twice.resolve(Some(&tx())),
            Err(Error::DuplicateSender { index: 1 })
        ));
        let incomplete = Hl7v2 {
            enabled: true,
            senders: vec![sender("LAB", " ")],
            ..Hl7v2::default()
        };
        assert!(matches!(
            incomplete.resolve(Some(&tx())),
            Err(Error::Value { .. })
        ));
        let zero = Hl7v2 {
            enabled: true,
            frame_limit: super::ByteSize::from_bytes(0),
            ..Hl7v2::default()
        };
        assert!(matches!(
            zero.resolve(Some(&tx())),
            Err(Error::Value { .. })
        ));
        let charset = Hl7v2 {
            enabled: true,
            default_charset: String::new(),
            ..Hl7v2::default()
        };
        assert!(matches!(
            charset.resolve(Some(&tx())),
            Err(Error::Value { .. })
        ));
        let unknown_charset = Hl7v2 {
            enabled: true,
            default_charset: String::from("EBCDIC"),
            ..Hl7v2::default()
        };
        assert!(matches!(
            unknown_charset.resolve(Some(&tx())),
            Err(Error::Value { .. })
        ));
    }

    /// One change to a valid section.
    type Change = Box<dyn Fn(&mut Hl7v2)>;

    fn consumer(name: &str) -> Consumer {
        Consumer {
            name: name.to_owned(),
            address: String::from("consumer.example.org:2575"),
            application: String::from("PIXCONSUMER"),
            facility: String::from("WEST"),
            domains: vec![String::from("urn:oid:2.999.1")],
            ..Consumer::default()
        }
    }

    #[test]
    fn consumers_resolve_and_a_faulty_one_is_refused_by_its_key() {
        let on = Hl7v2 {
            enabled: true,
            consumers: vec![consumer("west"), {
                let mut every = consumer("every");
                every.domains.clear();
                every
            }],
            ..Hl7v2::default()
        };
        let resolved = on.resolve(Some(&tx())).expect("resolves").expect("on");
        assert_eq!(2, resolved.consumers.len());
        assert!(resolved.consumers[0].wants("urn:oid:2.999.1"));
        assert!(!resolved.consumers[0].wants("urn:oid:2.999.2"));
        assert!(
            resolved.consumers[1].wants("urn:oid:2.999.2"),
            "every domain"
        );
        assert_eq!(
            (String::from("FERROPIX"), String::from("FERROPIX")),
            resolved.manager
        );
        let cases: Vec<(&str, Change)> = vec![
            (
                "hl7v2.consumers[1].name",
                Box::new(|h| h.consumers.push(consumer("west"))),
            ),
            (
                "hl7v2.consumers[0].address",
                Box::new(|h| h.consumers[0].address = String::from("consumer.example.org")),
            ),
            (
                "hl7v2.consumers[0].address",
                Box::new(|h| h.consumers[0].address = String::from(":2575")),
            ),
            (
                "hl7v2.consumers[0].application",
                Box::new(|h| h.consumers[0].application = String::new()),
            ),
            (
                "hl7v2.consumers[0].domains",
                Box::new(|h| h.consumers[0].domains.push(String::from(" "))),
            ),
            (
                "hl7v2.consumers[0].reply_timeout",
                Box::new(|h| h.consumers[0].reply_timeout = super::Span::from_secs(0)),
            ),
            (
                "hl7v2.notification.max_backoff",
                Box::new(|h| h.notification.max_backoff = super::Span::from_millis(1)),
            ),
            (
                "hl7v2.notification.max_attempts",
                Box::new(|h| h.notification.max_attempts = 0),
            ),
            (
                "hl7v2.application",
                Box::new(|h| h.application = String::new()),
            ),
        ];
        for (wanted, change) in cases {
            let mut faulty = Hl7v2 {
                enabled: true,
                consumers: vec![consumer("west")],
                ..Hl7v2::default()
            };
            change(&mut faulty);
            assert!(
                matches!(faulty.resolve(Some(&tx())), Err(Error::Value { ref key, .. }) if key == wanted),
                "{wanted}"
            );
        }
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
            two.resolve(Some(&tx())),
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
            .resolve(Some(&tx()))
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
        assert!(matches!(
            shared.resolve(Some(&tx())),
            Err(Error::Value { .. })
        ));
    }
}
