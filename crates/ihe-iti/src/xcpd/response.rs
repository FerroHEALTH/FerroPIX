// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Reading the Cross Gateway Patient Discovery Response: a
//! `PRPA_IN201306UV02` or a SOAP 1.2 fault, held to the five cases of
//! §3.55.4.2.3.

use http::StatusCode;
use quick_xml::NsReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use secrecy::SecretString;

use super::discovery::{Discovery, Match, RequestedAttribute};
use super::error::{DetectedIssue, FaultCode, Malformation, XcpdError};
use super::identifier::{CommunityPatientId, HomeCommunityId, Oid};
use super::{HL7, REQUEST_SYSTEM, SOAP, WSA};

/// The most bytes of an answer the client reads (no specification governs
/// this: our own design, room for many matches and their demographics).
pub(super) const LIMIT: usize = 4 * 1024 * 1024;

/// The deepest element nesting the client reads (no specification governs
/// this: our own design; a response nests about a dozen deep).
const DEPTH: usize = 64;

/// What one registration event names.
#[derive(Default)]
struct Event1 {
    community: Option<String>,
    ids: Vec<CommunityPatientId>,
}

/// What the reader collects of an answer.
#[derive(Default)]
struct Read {
    envelope: bool,
    body: bool,
    fault: Option<String>,
    response: bool,
    interaction: Option<String>,
    relates_to: Option<String>,
    acknowledgement: Option<String>,
    query_response: Option<String>,
    events: Vec<Event1>,
    requested: Vec<RequestedAttribute>,
    issues: Vec<DetectedIssue>,
}

/// Which namespace an element is in, of the ones the reader looks at.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ns {
    Soap,
    Wsa,
    Hl7,
    Other,
}

/// One open element: its namespace and local name.
type Open = (Ns, String);

/// Reads the answer with `status`, media type `media` and `body` to the
/// request whose `MessageID` is `message`.
pub(super) fn read(
    status: StatusCode,
    media: Option<&str>,
    body: &[u8],
    message: &str,
) -> Result<Discovery, XcpdError> {
    let soap = match media.map(media_type) {
        Some(kind) if kind == "application/soap+xml" => true,
        Some(kind) if kind == "multipart/related" => return Err(Malformation::Multipart.into()),
        _ => false,
    };
    if !soap {
        return Err(if status == StatusCode::OK {
            Malformation::NotSoap.into()
        } else {
            XcpdError::Rejected { status }
        });
    }
    let read = parse(body)?;
    if let Some(code) = read.fault {
        return Err(XcpdError::Fault {
            code: FaultCode::of(&code),
        });
    }
    if status != StatusCode::OK {
        return Err(XcpdError::Rejected { status });
    }
    if !read.response {
        return Err(Malformation::UnexpectedBody.into());
    }
    if let Some(relates_to) = &read.relates_to
        && relates_to.trim() != message
    {
        return Err(Malformation::RelatesToAnother.into());
    }
    if read.interaction.as_deref() != Some("PRPA_IN201306UV02") {
        return Err(Malformation::WrongInteraction.into());
    }
    decide(read)
}

/// The answer under the five cases of §3.55.4.2.3.
fn decide(read: Read) -> Result<Discovery, XcpdError> {
    match read.acknowledgement.as_deref() {
        Some("AA") => {}
        Some("AE" | "AR") => {
            return Err(XcpdError::ApplicationError {
                issues: read.issues,
            });
        }
        _ => return Err(Malformation::Acknowledgement.into()),
    }
    match read.query_response.as_deref() {
        Some("OK") if !read.events.is_empty() => matches(read.events).map(Discovery::Matched),
        Some("OK") if !read.requested.is_empty() => {
            Ok(Discovery::MoreAttributesRequested(read.requested))
        }
        Some("OK") => Err(Malformation::EmptyMatch.into()),
        Some("NF") if read.events.is_empty() => Ok(Discovery::NoMatch),
        Some("NF") => Err(Malformation::MatchesWithoutMatch.into()),
        Some("AE") => Err(XcpdError::ApplicationError {
            issues: read.issues,
        }),
        Some("QE") => Err(XcpdError::QueryRefused),
        _ => Err(Malformation::QueryResponseCode.into()),
    }
}

/// Each registration event as a match: its community and patient ids
/// (§3.55.4.2.2.2, §3.55.4.2.2.4).
fn matches(events: Vec<Event1>) -> Result<Vec<Match>, XcpdError> {
    let mut found = Vec::with_capacity(events.len());
    for (event, read) in events.into_iter().enumerate() {
        let community = read
            .community
            .as_deref()
            .and_then(|root| Oid::new(root).ok())
            .ok_or(Malformation::NoHomeCommunity { event })?;
        if read.ids.is_empty() {
            return Err(Malformation::NoPatientId { event }.into());
        }
        found.push(Match::new(HomeCommunityId::new(community), read.ids));
    }
    Ok(found)
}

/// The type and subtype of a media type, lowercased, without parameters.
fn media_type(media: &str) -> String {
    media
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// Collects what the answer says, refusing what is not well-formed XML.
fn parse(body: &[u8]) -> Result<Read, XcpdError> {
    let text = std::str::from_utf8(body).map_err(|error| Malformation::NotXml {
        position: u64::try_from(error.valid_up_to()).unwrap_or(u64::MAX),
    })?;
    let mut reader = NsReader::from_str(text);
    let mut open: Vec<Open> = Vec::new();
    let mut read = Read::default();
    let mut collected = String::new();
    loop {
        let (ns, event) = match reader.read_resolved_event() {
            Ok((namespace, event)) => (kind(&namespace), event),
            Err(_syntax) => {
                return Err(Malformation::NotXml {
                    position: reader.error_position(),
                }
                .into());
            }
        };
        match event {
            Event::Start(element) => {
                let local = local_name(&element);
                if open.len() >= DEPTH {
                    return Err(Malformation::TooDeep { limit: DEPTH }.into());
                }
                open.push((ns, local));
                element_opened(&mut read, &open, &element)?;
                collected.clear();
            }
            Event::Empty(element) => {
                let local = local_name(&element);
                open.push((ns, local));
                element_opened(&mut read, &open, &element)?;
                open.pop();
            }
            Event::End(_) => {
                element_closed(&mut read, &open, &collected);
                collected.clear();
                open.pop();
            }
            Event::Text(text) => collected.push_str(&text.xml10_content()),
            Event::CData(data) => {
                collected.push_str(&data.into_inner());
            }
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
                    .ok_or(Malformation::NotXml {
                        position: reader.buffer_position(),
                    })?;
                collected.push_str(&resolved);
            }
            Event::DocType(_) => return Err(Malformation::DocumentType.into()),
            Event::Eof => break,
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
        }
    }
    if !read.envelope || !read.body {
        return Err(Malformation::NotAnEnvelope.into());
    }
    Ok(read)
}

/// Which of the namespaces the reader looks at `namespace` is.
fn kind(namespace: &ResolveResult<'_>) -> Ns {
    match namespace {
        ResolveResult::Bound(ns) if ns.0 == SOAP => Ns::Soap,
        ResolveResult::Bound(ns) if ns.0 == WSA => Ns::Wsa,
        ResolveResult::Bound(ns) if ns.0 == HL7 => Ns::Hl7,
        _ => Ns::Other,
    }
}

fn local_name(element: &BytesStart<'_>) -> String {
    element.local_name().as_ref().to_owned()
}

/// The value of the attribute `name`, unescaped, if the element has it.
fn attribute(element: &BytesStart<'_>, name: &str) -> Result<Option<String>, XcpdError> {
    let malformed = Malformation::NotXml { position: 0 };
    let Some(found) = element
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

/// Whether the open elements, from the body's child down, are exactly
/// `path` in the HL7 v3 namespace.
fn at(open: &[Open], path: &[&str]) -> bool {
    let Some(inner) = open.get(3..) else {
        return false;
    };
    inner.len() == path.len()
        && inner
            .iter()
            .zip(path)
            .all(|((ns, local), expected)| *ns == Ns::Hl7 && local == expected)
}

/// What the element just opened says, given the elements open around it.
fn element_opened(
    read: &mut Read,
    open: &[Open],
    element: &BytesStart<'_>,
) -> Result<(), XcpdError> {
    let names: Vec<(Ns, &str)> = open
        .iter()
        .map(|(ns, local)| (*ns, local.as_str()))
        .collect();
    match names.as_slice() {
        [(Ns::Soap, "Envelope")] => read.envelope = true,
        [(_, _)] => return Err(Malformation::NotAnEnvelope.into()),
        [(Ns::Soap, "Envelope"), (Ns::Soap, "Body")] => read.body = true,
        [(Ns::Soap, "Envelope"), (Ns::Soap, "Body"), child] => match child {
            (Ns::Soap, "Fault") => read.fault = Some(String::new()),
            (Ns::Hl7, "PRPA_IN201306UV02") if !read.response => read.response = true,
            _ => return Err(Malformation::UnexpectedBody.into()),
        },
        _ => {}
    }
    if !read.response {
        return Ok(());
    }
    let reason_of = ["controlActProcess", "reasonOf", "detectedIssueEvent"];
    if at(open, &["interactionId"]) {
        read.interaction = attribute(element, "extension")?;
    } else if at(open, &["acknowledgement", "typeCode"]) {
        read.acknowledgement = attribute(element, "code")?;
    } else if at(
        open,
        &["controlActProcess", "queryAck", "queryResponseCode"],
    ) {
        read.query_response = attribute(element, "code")?;
    } else if at(open, &["controlActProcess", "subject", "registrationEvent"]) {
        read.events.push(Event1::default());
    } else if at(
        open,
        &[
            "controlActProcess",
            "subject",
            "registrationEvent",
            "subject1",
            "patient",
            "id",
        ],
    ) {
        if let (Some(event), Some(root)) = (read.events.last_mut(), attribute(element, "root")?) {
            let extension = attribute(element, "extension")?.map(SecretString::from);
            event
                .ids
                .push(CommunityPatientId::new(SecretString::from(root), extension));
        }
    } else if at(
        open,
        &[
            "controlActProcess",
            "subject",
            "registrationEvent",
            "custodian",
            "assignedEntity",
            "id",
        ],
    ) {
        if let Some(event) = read.events.last_mut() {
            event.community = attribute(element, "root")?;
        }
    } else if at(
        open,
        &[&reason_of[..], &["triggerFor", "actOrderRequired", "code"]].concat(),
    ) {
        if attribute(element, "codeSystem")?.as_deref() == Some(REQUEST_SYSTEM)
            && let Some(code) = attribute(element, "code")?
        {
            read.requested.push(RequestedAttribute::of(&code));
        }
    } else if at(
        open,
        &[
            &reason_of[..],
            &["mitigatedBy", "detectedIssueManagement", "code"],
        ]
        .concat(),
    ) && let Some(code) = attribute(element, "code")?
    {
        let system = attribute(element, "codeSystem")?;
        read.issues
            .push(DetectedIssue::of(&code, system.as_deref()));
    }
    Ok(())
}

/// What the element about to close held as text, given the elements open.
fn element_closed(read: &mut Read, open: &[Open], text: &str) {
    let names: Vec<(Ns, &str)> = open
        .iter()
        .map(|(ns, local)| (*ns, local.as_str()))
        .collect();
    match names.as_slice() {
        [
            (Ns::Soap, "Envelope"),
            (Ns::Soap, "Header"),
            (Ns::Wsa, "RelatesTo"),
        ] => {
            read.relates_to = Some(text.to_owned());
        }
        [
            (Ns::Soap, "Envelope"),
            (Ns::Soap, "Body"),
            (Ns::Soap, "Fault"),
            (Ns::Soap, "Code"),
            (Ns::Soap, "Value"),
        ] => read.fault = Some(text.trim().to_owned()),
        _ => {}
    }
}
