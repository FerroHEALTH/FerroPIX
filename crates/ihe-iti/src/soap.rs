// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The SOAP 1.2 envelope of an IHE Web Services transaction (feature
//! `soap`).
//!
//! ITI TF-2 Appendix V builds every Web Services transaction on SOAP 1.2
//! (§V.2.1) with WS-Addressing (§V.2.2). [`read`] reads one envelope: it
//! refuses what is not well-formed XML, a document type declaration (SOAP 1.2
//! Part 1 §5) and a nesting deeper than its caller reads, requires an
//! `Envelope` root with a `Body` (§5.1, §5.3), and reads the code of a
//! `Fault` (§5.4.6). What a transaction puts in the header and the body is
//! read by the [`Visitor`] its caller supplies, which [`read`] hands every
//! element: XCPD's HL7 v3 messages (feature `xcpd`) are read this way, and so
//! is any other Appendix V transaction a crate defines.

use std::time::Duration;

use ferrohealth_smart::client::outbound::OutboundClient;
use http::header::{ACCEPT, CONTENT_TYPE};
use http::{Method, StatusCode};
use quick_xml::events::{BytesDecl, BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::{NsReader, Writer};
use url::Url;

use crate::exchange;
use crate::outbound::{self, Unauthenticated};

/// The SOAP 1.2 envelope namespace (SOAP 1.2 Part 1 §5).
pub const ENVELOPE: &str = "http://www.w3.org/2003/05/soap-envelope";

/// The WS-Addressing 1.0 namespace (ITI TF-2 §V.2.2).
pub const ADDRESSING: &str = "http://www.w3.org/2005/08/addressing";

/// The WS-Addressing 1.0 anonymous address: the reply comes back on the
/// request's own connection (WS-Addressing 1.0 Core §2.1).
pub const ANONYMOUS: &str = "http://www.w3.org/2005/08/addressing/anonymous";

/// The media type of a SOAP 1.2 message (RFC 3902), without parameters.
pub const MEDIA_TYPE: &str = "application/soap+xml";

/// What reads the header and the body of an envelope, element by element.
///
/// [`read`] calls [`Visitor::opened`] for every element as it opens, the
/// envelope's own included, and [`Visitor::closed`] as it closes, each with
/// the elements open from the root down to it as namespace and local name.
pub trait Visitor {
    /// The namespaces the visitor tells apart.
    type Namespace: Copy + PartialEq + 'static;
    /// The error the visitor refuses a message with.
    type Error: From<Malformed>;
    /// The SOAP 1.2 envelope namespace, as the visitor names it.
    const SOAP: Self::Namespace;
    /// Every namespace the visitor reads besides [`ENVELOPE`], by its URI.
    const NAMESPACES: &[(&str, Self::Namespace)];
    /// Any other namespace, and no namespace.
    const OTHER: Self::Namespace;

    /// Holds the media type `media` of a message to the transaction's.
    ///
    /// # Errors
    ///
    /// [`Malformed::NotSoap`] unless `media` is [`MEDIA_TYPE`], the default.
    fn media(media: Option<&str>) -> Result<(), Self::Error> {
        if media.is_some_and(|media| essence(media) == MEDIA_TYPE) {
            Ok(())
        } else {
            Err(Malformed::NotSoap.into())
        }
    }

    /// Reads the element that just opened, the last of `open`.
    ///
    /// # Errors
    ///
    /// The visitor's error for an element its transaction refuses.
    fn opened(
        &mut self,
        open: &[(Self::Namespace, &str)],
        element: &Element<'_>,
    ) -> Result<(), Self::Error>;

    /// Reads the element about to close, the last of `open`, at byte `at`
    /// past its end, with the text it held; `None` for an element written as
    /// an empty-element tag.
    fn closed(&mut self, open: &[(Self::Namespace, &str)], text: Option<&str>, at: usize);
}

/// An element as it opens, whose attributes a [`Visitor`] reads.
pub struct Element<'a> {
    /// The start tag.
    start: &'a BytesStart<'a>,
    /// The byte of the message the start tag begins at.
    at: usize,
}

impl std::fmt::Debug for Element<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Element").finish_non_exhaustive()
    }
}

impl Element<'_> {
    /// The byte of the message the element's start tag begins at.
    #[must_use]
    pub const fn at(&self) -> usize {
        self.at
    }

    /// The value of the attribute `name`, unescaped, if the element has it.
    ///
    /// # Errors
    ///
    /// [`Malformed::NotXml`] for an attribute that is not well-formed.
    pub fn attribute(&self, name: &str) -> Result<Option<String>, Malformed> {
        let malformed = Malformed::NotXml { position: 0 };
        let Some(found) = self
            .start
            .try_get_attribute(name)
            .map_err(|_attribute| malformed.clone())?
        else {
            return Ok(None);
        };
        let value = found
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|_escape| malformed)?;
        Ok(Some(value.into_owned()))
    }
}

/// How a message departs from a SOAP 1.2 envelope.
///
/// No variant carries a value from the message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Malformed {
    /// The message's media type is not [`MEDIA_TYPE`].
    #[error("the message is not application/soap+xml")]
    NotSoap,
    /// The message is not well-formed XML.
    #[error("the message is not well-formed XML (byte {position})")]
    NotXml {
        /// The byte offset of the fault.
        position: u64,
    },
    /// The message carries a document type declaration, which a SOAP
    /// message must not (SOAP 1.2 Part 1 §5).
    #[error("the message carries a document type declaration")]
    DocumentType,
    /// The message nests deeper than its reader reads.
    #[error("the message nests deeper than {limit} elements")]
    TooDeep {
        /// The limit, in elements.
        limit: usize,
    },
    /// The message is not a SOAP 1.2 envelope with a body.
    #[error("the message is not a SOAP 1.2 envelope with a body")]
    NotAnEnvelope,
}

/// The code of a SOAP 1.2 fault (SOAP 1.2 Part 1 §5.4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FaultCode {
    /// `VersionMismatch`: the envelope is not in the SOAP 1.2 namespace.
    VersionMismatch,
    /// `MustUnderstand`: a header the receiver was required to understand,
    /// such as a WS-Addressing `Action` or a WS-Security header, was not.
    MustUnderstand,
    /// `DataEncodingUnknown`.
    DataEncodingUnknown,
    /// `Sender`: the message was incorrectly formed or carried wrong
    /// information.
    Sender,
    /// `Receiver`: the message could not be processed for reasons of the
    /// receiver.
    Receiver,
    /// A code SOAP 1.2 does not define.
    Other,
}

impl FaultCode {
    /// The code a fault's `Code/Value` names, by its local part.
    fn of(value: &str) -> Self {
        match value.rsplit(':').next().unwrap_or(value).trim() {
            "VersionMismatch" => Self::VersionMismatch,
            "MustUnderstand" => Self::MustUnderstand,
            "DataEncodingUnknown" => Self::DataEncodingUnknown,
            "Sender" => Self::Sender,
            "Receiver" => Self::Receiver,
            _ => Self::Other,
        }
    }
}

impl std::fmt::Display for FaultCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::VersionMismatch => "VersionMismatch",
            Self::MustUnderstand => "MustUnderstand",
            Self::DataEncodingUnknown => "DataEncodingUnknown",
            Self::Sender => "Sender",
            Self::Receiver => "Receiver",
            Self::Other => "a code SOAP 1.2 does not define",
        })
    }
}

/// An envelope [`read`] read.
#[derive(Debug)]
pub struct Envelope<'a> {
    /// The message as text.
    text: &'a str,
    /// The code of the body's fault, when the body holds one.
    fault: Option<FaultCode>,
}

impl<'a> Envelope<'a> {
    /// The message as text, for a visitor that kept byte offsets into it.
    #[must_use]
    pub const fn text(&self) -> &'a str {
        self.text
    }

    /// The code of the fault the body holds (SOAP 1.2 Part 1 §5.4), or
    /// `None` when it holds none.
    #[must_use]
    pub const fn fault(&self) -> Option<FaultCode> {
        self.fault
    }
}

/// An answer over the SOAP 1.2 HTTP binding, as [`answer`] read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// A well-formed envelope under `200` with no fault, its header and body
    /// read by the visitor.
    Envelope,
    /// The body is a fault, with the HTTP status it came with.
    Fault {
        /// The fault's code.
        code: FaultCode,
        /// The HTTP status.
        status: StatusCode,
    },
    /// The answer has a status other than `200` and holds no fault.
    Rejected {
        /// The HTTP status.
        status: StatusCode,
    },
}

/// Reads an answer with `status`, media type `media` and `body` as a SOAP
/// 1.2 envelope nesting at most `depth` elements, handing every element to
/// `visitor`.
///
/// A fault is the answer under any status. Under a status other than `200`
/// an answer that is no well-formed envelope, or one with no fault, is
/// [`Answer::Rejected`]: the status says enough.
///
/// # Errors
///
/// The visitor's error, under `200` only, for an answer that is no
/// well-formed SOAP 1.2 envelope with a body or that the visitor refuses.
pub fn answer<V: Visitor>(
    status: StatusCode,
    media: Option<&str>,
    body: &[u8],
    depth: usize,
    visitor: &mut V,
) -> Result<Answer, V::Error> {
    let read = V::media(media).and_then(|()| read(body, depth, visitor));
    match read {
        Ok(envelope) => Ok(match envelope.fault {
            Some(code) => Answer::Fault { code, status },
            None if status == StatusCode::OK => Answer::Envelope,
            None => Answer::Rejected { status },
        }),
        Err(_unread) if status != StatusCode::OK => Ok(Answer::Rejected { status }),
        Err(unread) => Err(unread),
    }
}

/// Writes a SOAP 1.2 envelope whose `Header` `header` writes and whose
/// `Body` `body` writes.
///
/// The envelope follows an XML declaration and binds the prefixes `soap` and
/// `wsa` to the envelope and WS-Addressing namespaces (SOAP 1.2 Part 1 §5,
/// ITI TF-2 §V.2.2).
///
/// # Errors
///
/// The writer's error, or the error `header` or `body` returns.
pub fn write(
    header: impl FnOnce(&mut Writer<Vec<u8>>) -> std::io::Result<()>,
    body: impl FnOnce(&mut Writer<Vec<u8>>) -> std::io::Result<()>,
) -> std::io::Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    writer
        .create_element("soap:Envelope")
        .with_attributes([("xmlns:soap", ENVELOPE), ("xmlns:wsa", ADDRESSING)])
        .write_inner_content(|writer| {
            writer
                .create_element("soap:Header")
                .write_inner_content(header)?;
            writer
                .create_element("soap:Body")
                .write_inner_content(body)?;
            Ok(())
        })?;
    Ok(writer.into_inner())
}

/// A client error that a SOAP 1.2 exchange over HTTP ends in, besides the
/// failures of [`Unanswered`](exchange::Unanswered) and [`Unauthenticated`].
pub trait Refusal: Unauthenticated {
    /// The endpoint answered `status`, a status other than `200`, with a
    /// body longer than the client reads.
    fn rejected(status: StatusCode) -> Self;

    /// The endpoint answered `200` with a body longer than `limit` bytes.
    fn too_large(limit: usize) -> Self;
}

/// Posts the SOAP 1.2 message `body` to `url` through `client` within
/// `timeout`, and reads the answer whole up to `limit` bytes.
///
/// The post carries the media type `media_type`, its action parameter
/// included (RFC 3902), and the headers `headers` adds. `client` tries the post again as its `retry` table allows, so `headers`
/// gives every try the same headers.
///
/// # Errors
///
/// [`Refusal::too_large`] for a `200` longer than `limit`,
/// [`Refusal::rejected`] for any other status that is,
/// [`failed`](exchange::Unanswered::failed) or [`timeout`](exchange::Unanswered::timeout) when no answer
/// arrives, and [`Unauthenticated::unauthenticated`] when no credential
/// could be had.
pub async fn post<E: Refusal>(
    client: &OutboundClient,
    url: &Url,
    media_type: &'static str,
    body: &[u8],
    timeout: Duration,
    limit: usize,
    headers: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
) -> Result<exchange::Answer, E> {
    let response = outbound::send::<E, _>(client, &Method::POST, url, timeout, |request| {
        headers(request)
            .header(CONTENT_TYPE, media_type)
            .header(ACCEPT, MEDIA_TYPE)
            .body(body.to_vec())
    })
    .await?;
    exchange::read(response, limit, |status| {
        if status == StatusCode::OK {
            E::too_large(limit)
        } else {
            E::rejected(status)
        }
    })
    .await
}

/// The type and subtype of the media type `header`, lowercased, without
/// parameters.
#[must_use]
pub fn essence(header: &str) -> String {
    header
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// Reads `body` as a SOAP 1.2 envelope nesting at most `depth` elements,
/// handing every element to `visitor`.
///
/// # Errors
///
/// A [`Malformed`], as the visitor's error, for a message that is not a
/// well-formed SOAP 1.2 envelope with a body, and the visitor's own error for
/// an element it refuses.
pub fn read<'a, V: Visitor>(
    body: &'a [u8],
    depth: usize,
    visitor: &mut V,
) -> Result<Envelope<'a>, V::Error> {
    let text = std::str::from_utf8(body).map_err(|error| Malformed::NotXml {
        position: u64::try_from(error.valid_up_to()).unwrap_or(u64::MAX),
    })?;
    let mut reader = NsReader::from_str(text);
    let mut open: Vec<(V::Namespace, String)> = Vec::new();
    let mut seen = Seen::default();
    let mut words = String::new();
    loop {
        let before = usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX);
        let (namespace, event) = match reader.read_resolved_event() {
            Ok((namespace, event)) => (resolve::<V>(&namespace), event),
            Err(_syntax) => {
                return Err(Malformed::NotXml {
                    position: reader.error_position(),
                }
                .into());
            }
        };
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(start) | Event::Empty(start) => {
                if !empty && open.len() >= depth {
                    return Err(Malformed::TooDeep { limit: depth }.into());
                }
                open.push((namespace, local_name(&start)));
                let names = names(&open);
                seen.opened::<V>(&names)?;
                let element = Element {
                    start: &start,
                    at: before,
                };
                visitor.opened(&names, &element)?;
                if empty {
                    visitor.closed(&names, None, before);
                    open.pop();
                } else {
                    words.clear();
                }
            }
            Event::End(_) => {
                let after = usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX);
                let names = names(&open);
                seen.closed::<V>(&names, &words);
                visitor.closed(&names, Some(&words), after);
                words.clear();
                open.pop();
            }
            Event::Text(text) => words.push_str(&text.xml10_content()),
            Event::CData(data) => words.push_str(&data.into_inner()),
            Event::GeneralRef(reference) => {
                let resolved = reference
                    .resolve_char_ref()
                    .ok()
                    .flatten()
                    .map(String::from)
                    .or_else(|| {
                        quick_xml::escape::resolve_predefined_entity(&reference.xml10_content())
                            .map(str::to_owned)
                    })
                    .ok_or(Malformed::NotXml {
                        position: reader.buffer_position(),
                    })?;
                words.push_str(&resolved);
            }
            Event::DocType(_) => return Err(Malformed::DocumentType.into()),
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
        }
    }
    if !seen.envelope || !seen.body {
        return Err(Malformed::NotAnEnvelope.into());
    }
    Ok(Envelope {
        text,
        fault: seen.fault.as_deref().map(FaultCode::of),
    })
}

/// What [`read`] has seen of the envelope itself.
#[derive(Default)]
struct Seen {
    /// The root is a SOAP `Envelope`.
    envelope: bool,
    /// The envelope has a `Body`.
    body: bool,
    /// The text of the body's fault code, empty until its `Value` closes.
    fault: Option<String>,
}

impl Seen {
    /// Notes the element that just opened, the last of `open`.
    fn opened<V: Visitor>(&mut self, open: &[(V::Namespace, &str)]) -> Result<(), Malformed> {
        if open.len() == 1 {
            if !in_soap::<V>(open, &["Envelope"]) {
                return Err(Malformed::NotAnEnvelope);
            }
            self.envelope = true;
        } else if in_soap::<V>(open, &["Envelope", "Body"]) {
            self.body = true;
        } else if in_soap::<V>(open, &["Envelope", "Body", "Fault"]) {
            self.fault = Some(String::new());
        }
        Ok(())
    }

    /// Notes the element about to close, the last of `open`, with `text`.
    fn closed<V: Visitor>(&mut self, open: &[(V::Namespace, &str)], text: &str) {
        if in_soap::<V>(open, &["Envelope", "Body", "Fault", "Code", "Value"]) {
            self.fault = Some(text.trim().to_owned());
        }
    }
}

/// Whether the open elements are exactly `path` in the SOAP namespace.
fn in_soap<V: Visitor>(open: &[(V::Namespace, &str)], path: &[&str]) -> bool {
    open.len() == path.len()
        && open
            .iter()
            .zip(path)
            .all(|((namespace, local), expected)| *namespace == V::SOAP && local == expected)
}

/// The open elements as namespace and local name.
fn names<N: Copy>(open: &[(N, String)]) -> Vec<(N, &str)> {
    open.iter()
        .map(|(namespace, local)| (*namespace, local.as_str()))
        .collect()
}

/// The namespace an element is in, as `V` names it.
fn resolve<V: Visitor>(namespace: &ResolveResult<'_>) -> V::Namespace {
    match namespace {
        ResolveResult::Bound(ns) if ns.0 == ENVELOPE => V::SOAP,
        ResolveResult::Bound(ns) => V::NAMESPACES
            .iter()
            .find(|(uri, _)| *uri == ns.0)
            .map_or(V::OTHER, |(_, namespace)| *namespace),
        _ => V::OTHER,
    }
}

/// The local name of the element `start` opens.
fn local_name(start: &BytesStart<'_>) -> String {
    start.local_name().as_ref().to_owned()
}
