// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The segments an answer of the face writes, in the delimiters of the
//! message it answers.
//!
//! An answer is an acknowledgment (`ACK`) or a query response (`RSP`): its
//! header swaps the sending and receiving application and facility of the
//! message (MSH-3 to MSH-6), keeps its processing id, version and character
//! set, and carries a control id and a time of its own; MSA-2 echoes MSH-10
//! (ITI TF-2 §3.9.4.2.2.2 and §3.21.4.2.2.2 refer to the acknowledgment
//! modes for the MSA). Every value the face writes is escaped with the
//! message's own delimiters, and a value read from the message is written
//! back as it was read, escaped again. No specification governs how a value
//! that holds a carriage return is written: our own design writes a space.

use ferrobridge_hl7v2::parse::{Component, Delimiters, Field, Location, Message, Repetition};

use crate::hl7v2::stamped::Stamped;

/// The table 0357 code an `ERR` segment carries in ERR-3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    /// `101`, required field missing.
    RequiredFieldMissing,
    /// `102`, data type error.
    DataType,
    /// `200`, unsupported message type.
    UnsupportedMessageType,
    /// `204`, unknown key identifier (ITI TF-2 §3.9.4.2.2.6, Cases 3 to 5).
    UnknownKey,
    /// `207`, application error.
    Application,
}

impl Condition {
    /// Returns the code and its display.
    #[must_use]
    pub const fn code(self) -> (&'static str, &'static str) {
        match self {
            Self::RequiredFieldMissing => ("101", "Required field missing"),
            Self::DataType => ("102", "Data type error"),
            Self::UnsupportedMessageType => ("200", "Unsupported message type"),
            Self::UnknownKey => ("204", "Unknown key identifier"),
            Self::Application => ("207", "Application error"),
        }
    }
}

/// One `ERR` segment: where, which condition, and a text that quotes
/// nothing from the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    /// ERR-2, the location, when the fault has one.
    pub location: Option<Box<Location>>,
    /// ERR-3.
    pub condition: Condition,
    /// ERR-8, the user message.
    pub text: String,
}

impl Fault {
    /// A fault at `location`.
    #[must_use]
    pub fn at(location: Location, condition: Condition, text: impl Into<String>) -> Self {
        Self {
            location: Some(Box::new(location)),
            condition,
            text: text.into(),
        }
    }

    /// A fault with no location.
    #[must_use]
    pub fn anywhere(condition: Condition, text: impl Into<String>) -> Self {
        Self {
            location: None,
            condition,
            text: text.into(),
        }
    }
}

/// Returns the location of field `field` of the `sequence`th segment `id`,
/// narrowed to `repetition` and `component` when they are given.
#[must_use]
pub fn location(
    id: &str,
    sequence: usize,
    field: usize,
    repetition: Option<usize>,
    component: Option<usize>,
) -> Location {
    let mut location = Location::segment(id, sequence).with_field(field);
    location.repetition = repetition;
    location.component = component;
    location
}

/// Writes segments in the delimiters of one message.
#[derive(Debug)]
pub struct Writer<'m> {
    /// The delimiters of the message answered.
    delimiters: &'m Delimiters,
}

impl<'m> Writer<'m> {
    /// Returns the writer for answers to `message`.
    #[must_use]
    pub const fn of(message: &'m Message) -> Self {
        Self {
            delimiters: message.delimiters(),
        }
    }

    /// Returns `text` escaped to travel as one value.
    #[must_use]
    pub fn escape(&self, text: &str) -> String {
        self.delimiters
            .escape(text)
            .chars()
            .map(|character| match character {
                '\r' | '\n' => ' ',
                other => other,
            })
            .collect()
    }

    /// Returns the subcomponents `parts`, each escaped, joined as one
    /// component; trailing empty subcomponents are left out.
    #[must_use]
    pub fn subcomponents(&self, parts: &[&str]) -> String {
        Self::join(parts, self.delimiters.subcomponent, |part| {
            self.escape(part)
        })
    }

    /// Returns the components `parts`, already written, joined as one
    /// repetition; trailing empty components are left out.
    #[must_use]
    pub fn components(&self, parts: &[String]) -> String {
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
        Self::join(&borrowed, self.delimiters.component, str::to_owned)
    }

    /// Returns the repetitions `parts`, already written, joined as one
    /// field.
    #[must_use]
    pub fn repetitions(&self, parts: &[String]) -> String {
        parts.join(&self.delimiters.repetition.to_string())
    }

    /// Returns the segment `id` with the fields `fields`, already written;
    /// trailing empty fields are left out.
    #[must_use]
    pub fn segment(&self, id: &str, fields: &[String]) -> String {
        let mut parts: Vec<&str> = vec![id];
        parts.extend(fields.iter().map(String::as_str));
        while parts.len() > 1 && parts.last().is_some_and(|part| part.is_empty()) {
            parts.pop();
        }
        parts.join(&self.delimiters.field.to_string())
    }

    /// Returns `field` as read, written back escaped.
    #[must_use]
    pub fn field(&self, field: &Field) -> String {
        let repetitions: Vec<String> = field
            .repetitions()
            .iter()
            .map(|repetition| self.repetition(repetition))
            .collect();
        self.repetitions(&repetitions)
    }

    /// Returns `repetition` as read, written back escaped.
    fn repetition(&self, repetition: &Repetition) -> String {
        let components: Vec<String> = repetition
            .components()
            .iter()
            .map(|component| self.component(component))
            .collect();
        self.components(&components)
    }

    /// Returns `component` as read, written back escaped.
    fn component(&self, component: &Component) -> String {
        let parts: Vec<&str> = component
            .subcomponents()
            .iter()
            .map(String::as_str)
            .collect();
        self.subcomponents(&parts)
    }

    /// Returns the field at `position` of the segment `id` of `message`, as
    /// read and written back, or empty.
    #[must_use]
    pub fn echoed(&self, message: &Message, id: &str, position: usize) -> String {
        message
            .segments()
            .iter()
            .find(|segment| segment.id() == id)
            .and_then(|segment| segment.field(position))
            .map(|field| self.field(field))
            .unwrap_or_default()
    }

    /// Returns the first segment `id` of `message` as read, written back.
    #[must_use]
    pub fn echo_segment(&self, message: &Message, id: &str) -> Option<String> {
        let segment = message
            .segments()
            .iter()
            .find(|segment| segment.id() == id)?;
        let fields: Vec<String> = segment
            .fields()
            .iter()
            .map(|field| self.field(field))
            .collect();
        Some(self.segment(id, &fields))
    }

    /// Returns the header of an answer to `message`: the applications and
    /// facilities swapped, `message_type` (its components) in MSH-9, the
    /// stamp's time and control id, and MSH-11, MSH-12 and MSH-18 echoed.
    #[must_use]
    pub fn header(&self, message: &Message, message_type: &[&str], stamp: &Stamped) -> String {
        let echoed = |position: usize| self.echoed(message, "MSH", position);
        let message_type: Vec<String> = message_type.iter().map(|part| self.escape(part)).collect();
        let mut fields = vec![
            self.delimiters.encoding_characters(),
            echoed(5),
            echoed(6),
            echoed(3),
            echoed(4),
            self.escape(&stamp.timestamp),
            String::new(),
            self.components(&message_type),
            self.escape(&stamp.control_id),
            echoed(11),
            echoed(12),
        ];
        let charset = echoed(18);
        if !charset.is_empty() {
            fields.resize(16, String::new());
            fields.push(charset);
        }
        self.segment("MSH", &fields)
    }

    /// Returns the MSA segment of `code` for `message`.
    #[must_use]
    pub fn msa(&self, message: &Message, code: &str) -> String {
        self.segment(
            "MSA",
            &[
                code.to_owned(),
                self.escape(message.control_id().unwrap_or_default()),
            ],
        )
    }

    /// Returns the ERR segment of `fault`.
    #[must_use]
    pub fn err(&self, fault: &Fault) -> String {
        let location = fault
            .location
            .as_ref()
            .map(|location| location.erl(self.delimiters.component))
            .unwrap_or_default();
        let (code, display) = fault.condition.code();
        let condition = self.components(&[
            code.to_owned(),
            self.escape(display),
            String::from("HL70357"),
        ]);
        self.segment(
            "ERR",
            &[
                String::new(),
                location,
                condition,
                String::from("E"),
                String::new(),
                String::new(),
                String::new(),
                self.escape(&fault.text),
            ],
        )
    }

    /// Returns `segments` as one message's text, each closed by a carriage
    /// return.
    #[must_use]
    pub fn message(segments: &[String]) -> String {
        let mut text = segments.join("\r");
        text.push('\r');
        text
    }

    /// Joins `parts` with `separator`, each written by `write`, leaving out
    /// the trailing empty ones.
    fn join(parts: &[&str], separator: char, write: impl Fn(&str) -> String) -> String {
        let mut written: Vec<String> = parts.iter().map(|part| write(part)).collect();
        while written.last().is_some_and(String::is_empty) {
            written.pop();
        }
        written.join(&separator.to_string())
    }
}

#[cfg(test)]
mod tests {
    use ferrobridge_hl7v2::decode::Charset;
    use ferrobridge_hl7v2::parse::lex::lex;

    use super::{Condition, Fault, Writer, location};
    use crate::hl7v2::stamped::Stamped;

    const QUERY: &str = "MSH|^~\\&|CONSUMER|EAST|PIX|NORTH|20261025100000||QBP^Q23^QBP_Q21|Q-1|P|2.5||||||UNICODE UTF-8\rQPD|IHE PIX Query|TAG-1|A\\S\\1^^^EAST&2.999.1.1&ISO|^^^&2.999.1.2&ISO~^^^LAB\rRCP|I\r";

    fn stamp() -> Stamped {
        Stamped {
            control_id: String::from("ANSWER-1"),
            timestamp: String::from("20261025100001+0000"),
        }
    }

    #[test]
    fn the_header_swaps_the_parties_and_echoes_the_version_and_the_charset() {
        let lexed = lex(QUERY, Charset::Utf8).expect("the query lexes");
        let writer = Writer::of(&lexed.message);
        assert_eq!(
            writer.header(&lexed.message, &["RSP", "K23", "RSP_K23"], &stamp()),
            "MSH|^~\\&|PIX|NORTH|CONSUMER|EAST|20261025100001+0000||RSP^K23^RSP_K23|ANSWER-1|P|2.5||||||UNICODE UTF-8"
        );
        assert_eq!(writer.msa(&lexed.message, "AA"), "MSA|AA|Q-1");
    }

    #[test]
    fn a_segment_read_is_written_back_with_its_escapes() {
        let lexed = lex(QUERY, Charset::Utf8).expect("the query lexes");
        let writer = Writer::of(&lexed.message);
        assert_eq!(
            writer.echo_segment(&lexed.message, "QPD").as_deref(),
            Some("QPD|IHE PIX Query|TAG-1|A\\S\\1^^^EAST&2.999.1.1&ISO|^^^&2.999.1.2&ISO~^^^LAB")
        );
    }

    #[test]
    fn an_err_names_its_location_and_table_0357_code() {
        let lexed = lex(QUERY, Charset::Utf8).expect("the query lexes");
        let writer = Writer::of(&lexed.message);
        let fault = Fault::at(
            location("QPD", 1, 4, Some(2), None),
            Condition::UnknownKey,
            "a domain of QPD-4 is not recognized",
        );
        assert_eq!(
            writer.err(&fault),
            "ERR||QPD^1^4^2|204^Unknown key identifier^HL70357|E||||a domain of QPD-4 is not recognized"
        );
        let anywhere = Fault::anywhere(Condition::Application, "a | b");
        assert_eq!(
            writer.err(&anywhere),
            "ERR|||207^Application error^HL70357|E||||a \\F\\ b"
        );
    }

    #[test]
    fn a_value_with_a_line_break_travels_as_one_value() {
        let lexed = lex(QUERY, Charset::Utf8).expect("the query lexes");
        let writer = Writer::of(&lexed.message);
        assert_eq!(writer.escape("a\rb^c"), "a b\\S\\c");
        assert_eq!(
            writer.subcomponents(&["", "2.999", "ISO", ""]),
            "&2.999&ISO"
        );
    }
}
