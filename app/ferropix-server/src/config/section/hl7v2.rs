// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `[hl7v2]` section: the MLLP listener, the character set of a message
//! with an empty MSH-18, the frame ceiling, the timeouts and the accepted
//! senders.
//!
//! A sender is named by MSH-3 and MSH-4, the sending application and
//! facility, and is the v2 counterpart of a Source's client id: it names the
//! identifier domain it feeds. No specification governs the listener: our own
//! design.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::time::Duration;

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
    /// The system of the identifier domain the sender feeds.
    pub domain: String,
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
        }
    }
}

/// The `[hl7v2]` section, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hl7v2Settings {
    /// The socket address the listener binds.
    pub listen: SocketAddr,
    /// The HL7 table 0211 code a message with an empty MSH-18 is read in.
    pub default_charset: String,
    /// The largest message one frame may carry.
    pub frame_limit: usize,
    /// How long a connection may sit idle between frames.
    pub idle_timeout: Duration,
    /// How long one frame may take to arrive once it started.
    pub frame_timeout: Duration,
    /// The senders accepted.
    pub senders: Vec<Sender>,
}

impl Hl7v2 {
    /// Resolves the section, `None` when the listener is off.
    ///
    /// # Errors
    /// Returns [`Error::Listen`] for a listener that does not parse,
    /// [`Error::Value`] for an empty charset, a bound of 0 or a sender without
    /// its application, facility or domain, and [`Error::DuplicateSender`] for
    /// a sender named twice.
    pub fn resolve(&self) -> Result<Option<Hl7v2Settings>, Error> {
        if !self.enabled {
            return Ok(None);
        }
        let listen = socket_address("hl7v2.listen", &self.listen)?;
        // TODO(#85): resolve the code through the MSH-18 decoder of ferrobridge-hl7v2.
        if self.default_charset.trim().is_empty() {
            return Err(value("hl7v2.default_charset", "an HL7 table 0211 code"));
        }
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
        for (index, sender) in self.senders.iter().enumerate() {
            if [&sender.application, &sender.facility, &sender.domain]
                .iter()
                .any(|field| field.trim().is_empty())
            {
                return Err(Error::Value {
                    key: format!("hl7v2.senders[{index}]"),
                    rule: "a sender names its application, facility and domain",
                });
            }
            if !seen.insert((sender.application.as_str(), sender.facility.as_str())) {
                return Err(Error::DuplicateSender { index });
            }
        }
        Ok(Some(Hl7v2Settings {
            listen,
            default_charset: self.default_charset.clone(),
            frame_limit: self.frame_limit_bytes,
            idle_timeout: Duration::from_millis(self.idle_timeout_ms),
            frame_timeout: Duration::from_millis(self.frame_timeout_ms),
            senders: self.senders.clone(),
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
        let on = Hl7v2 {
            enabled: true,
            senders: vec![sender("LAB", "EAST"), sender("LAB", "WEST")],
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
    }
}
