// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-10, the PIX Update Notification, as the Manager writes it and reads
//! the consumer's acknowledgment.
//!
//! The message is ADT^A31 with MSH, EVN, PID and PV1 (ITI TF-2 §3.10.4.1.2,
//! Table 3.10-1), in HL7 v2.5 (§3.10.3). MSH-9 carries all three components
//! `ADT^A31^ADT_A05` (§3.10.4.1.2.1). PID-3 holds one repetition per
//! identifier, each with all three subcomponents of its assigning authority,
//! and PID-5 a single space (§3.10.4.1.2.3). PV1-2 is `N` and every other PV1
//! field is empty (§3.10.4.1.2.4). EVN-2, the one EVN field the 2.5 tables of
//! `hl7v2-types` require, is the time of the notification. Each message is
//! acknowledged with an ACK (§3.10.4.1.2): MSA-1 `AA` is a delivery, any
//! other code a refusal, and an MSA-2 that names another message is no
//! acknowledgment of this one.

use ferrobridge_hl7v2::decode::{Charset, decode};
use ferrobridge_hl7v2::parse::Delimiters;
use ferrobridge_hl7v2::parse::lex::lex;

use crate::hl7v2::authority::{Authorities, Hd};
use crate::hl7v2::pid::{self, Shown};
use crate::hl7v2::stamped::Stamped;
use crate::hl7v2::write::Writer;

/// The delimiters a notification declares in MSH-1 and MSH-2. No
/// specification governs the choice, which the message declares: our own
/// design writes the ones every message of the face's tests uses.
const DELIMITERS: Delimiters = Delimiters {
    field: '|',
    component: '^',
    repetition: '~',
    escape: '\\',
    subcomponent: '&',
    truncation: None,
};

/// The HL7 version of ITI-10 (ITI TF-2 §3.10.3).
const VERSION: &str = "2.5";

/// An application and its facility, as MSH names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Party<'a> {
    /// The application, MSH-3 or MSH-5.
    pub application: &'a str,
    /// The facility, MSH-4 or MSH-6.
    pub facility: &'a str,
}

impl Party<'_> {
    /// Returns the facility and the application joined by `|`, the user id
    /// the audit message names a party by (ITI TF-2 §3.10.5.1.1).
    #[must_use]
    pub fn audit_id(&self) -> String {
        format!("{}|{}", self.facility, self.application)
    }
}

/// Writes the ADT^A31 that notifies `consumer` of `identifiers`, each a
/// domain's system and a value, from `manager`, stamped `stamped`.
///
/// A message whose text is not all ASCII declares `UNICODE UTF-8` in MSH-18
/// and travels as UTF-8; one that is leaves MSH-18 empty, read as ASCII.
#[must_use]
pub fn adt_a31(
    manager: Party<'_>,
    consumer: Party<'_>,
    authorities: &Authorities,
    identifiers: &[(String, String)],
    stamped: &Stamped,
) -> String {
    let writer = Writer::with(&DELIMITERS);
    let ascii = identifiers
        .iter()
        .all(|(system, value)| system.is_ascii() && value.is_ascii())
        && [manager, consumer]
            .iter()
            .all(|party| party.application.is_ascii() && party.facility.is_ascii());
    let mut header = vec![
        writer.encoding_characters(),
        writer.escape(manager.application),
        writer.escape(manager.facility),
        writer.escape(consumer.application),
        writer.escape(consumer.facility),
        writer.escape(&stamped.timestamp),
        String::new(),
        writer.components(&[
            String::from("ADT"),
            String::from("A31"),
            String::from("ADT_A05"),
        ]),
        writer.escape(&stamped.control_id),
        String::from("P"),
        String::from(VERSION),
    ];
    if !ascii {
        header.resize(16, String::new());
        header.push(Charset::Utf8.code().to_owned());
    }
    let segments = [
        writer.segment("MSH", &header),
        writer.segment(
            "EVN",
            &[String::from("A31"), writer.escape(&stamped.timestamp)],
        ),
        pid::segment(&writer, authorities, 1, identifiers, None, Shown::Space),
        writer.segment("PV1", &[String::new(), String::from("N")]),
    ];
    Writer::message(&segments)
}

/// What the Manager reads back from a notification it wrote, for its audit
/// record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// MSH-10.
    pub control_id: String,
    /// MSH-5 and MSH-6, the consumer's application and facility.
    pub consumer: (String, String),
    /// PID-3: each identifier's system and value; an assigning authority
    /// the configuration names no domain by is left out.
    pub identifiers: Vec<(String, String)>,
}

/// Reads `text`, a notification [`adt_a31`] wrote, `None` when it does not
/// lex.
#[must_use]
pub fn read(text: &str, authorities: &Authorities) -> Option<Written> {
    let lexed = lex(text, Charset::Utf8).ok()?;
    let message = lexed.message;
    let header = message.header()?;
    let field = |position: usize| header.text(position).unwrap_or_default().to_owned();
    let identifiers = message
        .segments()
        .iter()
        .find(|segment| segment.id() == "PID")
        .and_then(|pid| pid.field(3))
        .map(|field| {
            field
                .repetitions()
                .iter()
                .filter_map(|cx| {
                    let value = cx.component(1)?.text()?.to_owned();
                    let system = authorities.system(&Hd::of(cx.component(4)))?;
                    Some((system, value))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Written {
        control_id: message.control_id()?.to_owned(),
        consumer: (field(5), field(6)),
        identifiers,
    })
}

/// How a consumer answered a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acknowledged {
    /// MSA-1 `AA` for this notification.
    Accepted,
    /// The consumer answered with another acknowledgment code.
    Refused(String),
    /// The reply is no acknowledgment of this notification: it does not
    /// decode or lex, carries no MSA, or its MSA-2 names another message.
    Unrelated(&'static str),
}

/// Reads `reply`, the consumer's answer to the notification `control_id`.
#[must_use]
pub fn acknowledged(reply: &[u8], control_id: &str) -> Acknowledged {
    let Ok(decoded) = decode(reply, Charset::Ascii) else {
        return Acknowledged::Unrelated("the reply does not decode in the set it declares");
    };
    let Ok(lexed) = lex(&decoded.text, decoded.charset) else {
        return Acknowledged::Unrelated("the reply is no HL7 v2 message");
    };
    let Some(msa) = lexed
        .message
        .segments()
        .iter()
        .find(|segment| segment.id() == "MSA")
    else {
        return Acknowledged::Unrelated("the reply carries no MSA segment");
    };
    if msa.text(2) != Some(control_id) {
        return Acknowledged::Unrelated("the acknowledgment's MSA-2 names another message");
    }
    match msa.text(1) {
        Some("AA") => Acknowledged::Accepted,
        Some(code) => Acknowledged::Refused(code.to_owned()),
        None => Acknowledged::Unrelated("the acknowledgment carries no MSA-1"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ferrobridge_hl7v2::decode::Charset;
    use ferrobridge_hl7v2::inbound::{self, Received};
    use ferrobridge_hl7v2::parse::structure::structure_declared;

    use super::{Acknowledged, Party, acknowledged, adt_a31, read};
    use crate::hl7v2::authority::Authorities;
    use crate::hl7v2::stamped::Stamped;

    const EAST: &str = "urn:oid:2.999.1.1";
    const WEST: &str = "https://west.example.org/patients";

    fn authorities() -> Authorities {
        Authorities::new(&BTreeMap::from([(EAST.to_owned(), String::from("EAST"))]))
    }

    fn stamp() -> Stamped {
        Stamped {
            control_id: String::from("NOTICE-1"),
            timestamp: String::from("20261026120000+0000"),
        }
    }

    fn written() -> String {
        adt_a31(
            Party {
                application: "FERROPIX",
                facility: "NORTH",
            },
            Party {
                application: "PIXCONSUMER",
                facility: "WEST",
            },
            &authorities(),
            &[
                (EAST.to_owned(), String::from("E-1")),
                (EAST.to_owned(), String::from("E-2")),
                (WEST.to_owned(), String::from("W|1")),
            ],
            &stamp(),
        )
    }

    #[test]
    fn the_notification_carries_the_segments_and_fields_iti_10_names() {
        assert_eq!(
            "MSH|^~\\&|FERROPIX|NORTH|PIXCONSUMER|WEST|20261026120000+0000||ADT^A31^ADT_A05|NOTICE-1|P|2.5\r\
             EVN|A31|20261026120000+0000\r\
             PID|1||E-1^^^EAST&2.999.1.1&ISO~E-2^^^EAST&2.999.1.1&ISO~W\\F\\1^^^&https://west.example.org/patients&URI|| \r\
             PV1||N\r",
            written()
        );
    }

    #[test]
    fn the_notification_parses_as_adt_a05_of_the_2_5_tables_with_no_refusal() {
        let text = written();
        let stamped = stamp();
        let Received::Parsed(inbound) = inbound::receive(
            text.as_bytes(),
            Charset::Ascii,
            structure_declared,
            stamped.stamp(),
        ) else {
            panic!("the notification parses");
        };
        assert_eq!("ADT_A05", inbound.parsed().structure_name());
        assert_eq!(0, inbound.parsed().refusals().len(), "no refusal");
    }

    #[test]
    fn a_value_outside_ascii_declares_utf_8() {
        let text = adt_a31(
            Party {
                application: "FERROPIX",
                facility: "NORTH",
            },
            Party {
                application: "PIXCONSUMER",
                facility: "WEST",
            },
            &authorities(),
            &[(EAST.to_owned(), String::from("É-1"))],
            &stamp(),
        );
        assert!(
            text.split('\r')
                .next()
                .is_some_and(|msh| msh.ends_with("|2.5||||||UNICODE UTF-8")),
            "{text}"
        );
    }

    #[test]
    fn the_manager_reads_back_what_it_wrote() {
        let read = read(&written(), &authorities()).expect("it reads");
        assert_eq!("NOTICE-1", read.control_id);
        assert_eq!(
            (String::from("PIXCONSUMER"), String::from("WEST")),
            read.consumer
        );
        assert_eq!(
            vec![
                (EAST.to_owned(), String::from("E-1")),
                (EAST.to_owned(), String::from("E-2")),
                (WEST.to_owned(), String::from("W|1")),
            ],
            read.identifiers
        );
    }

    #[test]
    fn only_an_aa_for_this_message_is_a_delivery() {
        let ack = |code: &str, id: &str| {
            format!(
                "MSH|^~\\&|PIXCONSUMER|WEST|FERROPIX|NORTH|20261026120001||ACK^A31^ACK|A-1|P|2.5\rMSA|{code}|{id}\r"
            )
        };
        assert_eq!(
            Acknowledged::Accepted,
            acknowledged(ack("AA", "NOTICE-1").as_bytes(), "NOTICE-1")
        );
        assert_eq!(
            Acknowledged::Refused(String::from("AE")),
            acknowledged(ack("AE", "NOTICE-1").as_bytes(), "NOTICE-1")
        );
        assert!(matches!(
            acknowledged(ack("AA", "NOTICE-2").as_bytes(), "NOTICE-1"),
            Acknowledged::Unrelated(_)
        ));
        assert!(matches!(
            acknowledged(b"garbage", "NOTICE-1"),
            Acknowledged::Unrelated(_)
        ));
    }
}
