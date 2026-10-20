// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The RFC 5424 syslog message an ITI-20 audit message travels in, and the
//! RFC 5425 §4.3 octet-counted frame it is written to a connection in.
//!
//! ITI TF-2 §3.20.4.1.2 fixes the header fields the transaction cares about:
//! the PRI is `<85>` (facility 10, security and authorization messages, at
//! severity 5, notice), the MSGID is `IHE+RFC-3881`, STRUCTURED-DATA is not
//! used, and the MSG is the audit message's XML in UTF-8 with no byte order
//! mark.

use std::fmt;

use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretSlice};

/// The PRI of every ITI-20 message: facility 10 at severity 5 (ITI TF-2
/// §3.20.4.1.2).
pub const PRI: &str = "<85>";

/// The MSGID of every ITI-20 message (ITI TF-2 §3.20.4.1.2).
pub const MSGID: &str = "IHE+RFC-3881";

/// The default TCP port of syslog over TLS (RFC 5425 §4.1).
pub const TLS_PORT: u16 = 6514;

/// A header field that is not 1 to `limit` printable US-ASCII characters
/// (RFC 5424 §6.2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the syslog {field} is not 1 to {limit} printable US-ASCII characters")]
pub struct HeaderError {
    /// The header field, such as `HOSTNAME`.
    pub field: &'static str,
    /// Its longest length.
    pub limit: usize,
}

/// A header field value: 1 to `LIMIT` printable US-ASCII characters, no
/// space (RFC 5424 §6, `PRINTUSASCII`).
#[derive(Clone, PartialEq, Eq)]
struct Field(String);

impl Field {
    fn new(field: &'static str, limit: usize, value: &str) -> Result<Self, HeaderError> {
        let printable = value.bytes().all(|byte| (33..=126).contains(&byte));
        if value.is_empty() || value.len() > limit || !printable {
            return Err(HeaderError { field, limit });
        }
        Ok(Self(value.to_owned()))
    }
}

impl fmt::Debug for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

/// The header fields of the sender: who writes every message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    hostname: Field,
    app_name: Field,
    procid: Field,
}

impl Sender {
    /// The sender `hostname` (RFC 5424 §6.2.4, at most 255 characters),
    /// writing as `app_name` (§6.2.5, at most 48) from the process `procid`
    /// (§6.2.6, at most 128).
    ///
    /// # Errors
    ///
    /// A [`HeaderError`] naming the first field that is empty, too long, or
    /// holds a character other than printable US-ASCII.
    pub fn new(hostname: &str, app_name: &str, procid: &str) -> Result<Self, HeaderError> {
        Ok(Self {
            hostname: Field::new("HOSTNAME", 255, hostname)?,
            app_name: Field::new("APP-NAME", 48, app_name)?,
            procid: Field::new("PROCID", 128, procid)?,
        })
    }

    /// The `HOSTNAME`.
    #[must_use]
    pub fn hostname(&self) -> &str {
        &self.hostname.0
    }

    /// The RFC 5425 §4.3 frame of the syslog message carrying `xml`, written
    /// at `at`: `MSG-LEN SP SYSLOG-MSG`.
    #[must_use]
    pub fn frame(&self, at: Timestamp, xml: &SecretSlice<u8>) -> SecretSlice<u8> {
        // NOTE: RFC 5424 §6.2.3.1: TIME-SECFRAC holds at most six digits.
        let timestamp = at.strftime("%Y-%m-%dT%H:%M:%S%.6fZ").to_string();
        let header = format!(
            "{PRI}1 {timestamp} {} {} {} {MSGID} - ",
            self.hostname.0, self.app_name.0, self.procid.0
        );
        let body = xml.expose_secret();
        let length = header.len().saturating_add(body.len());
        let mut frame = format!("{length} ").into_bytes();
        frame.reserve(length);
        frame.extend_from_slice(header.as_bytes());
        frame.extend_from_slice(body);
        SecretSlice::from(frame)
    }
}

/// Whether `bytes` is one whole RFC 5425 §4.3 frame: a `MSG-LEN` of
/// `NONZERO-DIGIT *DIGIT`, a space, and exactly that many octets.
#[must_use]
pub fn is_frame(bytes: &[u8]) -> bool {
    let Some(space) = bytes.iter().position(|byte| *byte == b' ') else {
        return false;
    };
    let (length, rest) = bytes.split_at(space);
    let digits = !length.is_empty()
        && length.first() != Some(&b'0')
        && length.iter().all(u8::is_ascii_digit);
    digits
        && std::str::from_utf8(length)
            .ok()
            .and_then(|text| text.parse::<usize>().ok())
            .is_some_and(|declared| declared.saturating_add(1) == rest.len())
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use secrecy::{ExposeSecret, SecretSlice};

    use super::{HeaderError, Sender};

    #[test]
    fn a_frame_counts_the_octets_of_the_message_it_carries() {
        let sender = Sender::new("gateway.example.org", "ferrofed", "4242").expect("a sender");
        let at: Timestamp = "2026-10-04T01:02:03.123456789Z".parse().expect("a time");
        let xml = SecretSlice::from(b"<AuditMessage/>".to_vec());
        let frame = sender.frame(at, &xml);
        let text = std::str::from_utf8(frame.expose_secret()).expect("UTF-8");
        let message = "<85>1 2026-10-04T01:02:03.123456Z gateway.example.org ferrofed 4242 IHE+RFC-3881 - <AuditMessage/>";
        assert_eq!(format!("{} {message}", message.len()), text);
    }

    #[test]
    fn a_header_field_with_a_space_or_over_its_limit_is_refused() {
        assert_eq!(
            Err(HeaderError {
                field: "HOSTNAME",
                limit: 255
            }),
            Sender::new("gate way", "ferrofed", "1")
        );
        assert_eq!(
            Err(HeaderError {
                field: "APP-NAME",
                limit: 48
            }),
            Sender::new("gateway", &"a".repeat(49), "1")
        );
        assert!(Sender::new("gateway", "ferrofed", "").is_err());
    }
}
