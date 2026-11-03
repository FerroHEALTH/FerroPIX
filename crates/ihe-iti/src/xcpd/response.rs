// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Reading the Cross Gateway Patient Discovery Response: a
//! `PRPA_IN201306UV02` or a SOAP 1.2 fault, held to the five cases of
//! §3.55.4.2.3.

use http::StatusCode;
use secrecy::SecretString;

use super::discovery::{Discovery, Match, RequestedAttribute};
use super::error::{DetectedIssue, Malformation, XcpdError};
use super::identifier::{CommunityPatientId, HomeCommunityId, Oid};
use super::{HL7, REQUEST_SYSTEM};
use crate::soap::{self, Answer, Element, Visitor};

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

/// Reads the answer with `status`, media type `media` and `body` to the
/// request whose `MessageID` is `message`.
pub(super) fn read(
    status: StatusCode,
    media: Option<&str>,
    body: &[u8],
    message: &str,
) -> Result<Discovery, XcpdError> {
    let mut read = Read::default();
    match soap::answer(status, media, body, DEPTH, &mut read)? {
        Answer::Envelope => {}
        Answer::Fault { code, status } => return Err(XcpdError::Fault { code, status }),
        Answer::Rejected { status } => return Err(XcpdError::Rejected { status }),
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

/// Whether the open elements, from the body's child down, are exactly
/// `path` in the HL7 v3 namespace.
fn at(open: &[(Ns, &str)], path: &[&str]) -> bool {
    let Some(inner) = open.get(3..) else {
        return false;
    };
    inner.len() == path.len()
        && inner
            .iter()
            .zip(path)
            .all(|((ns, local), expected)| *ns == Ns::Hl7 && local == expected)
}

impl Visitor for Read {
    type Namespace = Ns;
    type Error = Malformation;
    const SOAP: Ns = Ns::Soap;
    const NAMESPACES: &'static [(&'static str, Ns)] =
        &[(soap::ADDRESSING, Ns::Wsa), (HL7, Ns::Hl7)];
    const OTHER: Ns = Ns::Other;

    fn media(media: Option<&str>) -> Result<(), Malformation> {
        match media.map(soap::essence).as_deref() {
            Some(soap::MEDIA_TYPE) => Ok(()),
            Some("multipart/related") => Err(Malformation::Multipart),
            _ => Err(soap::Malformed::NotSoap.into()),
        }
    }

    fn opened(&mut self, open: &[(Ns, &str)], element: &Element<'_>) -> Result<(), Malformation> {
        if let [(Ns::Soap, "Envelope"), (Ns::Soap, "Body"), child] = open {
            match child {
                (Ns::Soap, "Fault") => {}
                (Ns::Hl7, "PRPA_IN201306UV02") if !self.response => self.response = true,
                _ => return Err(Malformation::UnexpectedBody),
            }
        }
        if !self.response {
            return Ok(());
        }
        let reason_of = ["controlActProcess", "reasonOf", "detectedIssueEvent"];
        if at(open, &["interactionId"]) {
            self.interaction = element.attribute("extension")?;
        } else if at(open, &["acknowledgement", "typeCode"]) {
            self.acknowledgement = element.attribute("code")?;
        } else if at(
            open,
            &["controlActProcess", "queryAck", "queryResponseCode"],
        ) {
            self.query_response = element.attribute("code")?;
        } else if at(open, &["controlActProcess", "subject", "registrationEvent"]) {
            self.events.push(Event1::default());
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
            if let (Some(event), Some(root)) = (self.events.last_mut(), element.attribute("root")?)
            {
                let extension = element.attribute("extension")?.map(SecretString::from);
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
            if let Some(event) = self.events.last_mut() {
                event.community = element.attribute("root")?;
            }
        } else if at(
            open,
            &[&reason_of[..], &["triggerFor", "actOrderRequired", "code"]].concat(),
        ) {
            if element.attribute("codeSystem")?.as_deref() == Some(REQUEST_SYSTEM)
                && let Some(code) = element.attribute("code")?
            {
                self.requested.push(RequestedAttribute::of(&code));
            }
        } else if at(
            open,
            &[
                &reason_of[..],
                &["mitigatedBy", "detectedIssueManagement", "code"],
            ]
            .concat(),
        ) && let Some(code) = element.attribute("code")?
        {
            let system = element.attribute("codeSystem")?;
            self.issues
                .push(DetectedIssue::of(&code, system.as_deref()));
        }
        Ok(())
    }

    fn closed(&mut self, open: &[(Ns, &str)], text: Option<&str>, _at: usize) {
        if let (
            [
                (Ns::Soap, "Envelope"),
                (Ns::Soap, "Header"),
                (Ns::Wsa, "RelatesTo"),
            ],
            Some(text),
        ) = (open, text)
        {
            self.relates_to = Some(text.to_owned());
        }
    }
}
