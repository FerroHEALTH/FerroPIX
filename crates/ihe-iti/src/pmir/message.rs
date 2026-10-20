// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The ITI-93 message read against the PMIR profiles, and the response and
//! refusal written back (§2:3.93.4.1.2, §2:3.93.4.2.2).

use std::collections::BTreeSet;

use fhir_types::codec::{Json, Object, Path, Value};
use fhir_types::r4::bundle::{Bundle, BundleEntry, BundleEntryRequest};
use fhir_types::r4::message_header::{
    MessageHeader, MessageHeaderEvent, MessageHeaderResponse, MessageHeaderSource,
};
use fhir_types::r4::operation_outcome::{OperationOutcome, OperationOutcomeIssue};
use fhir_types::r4::patient::Patient;
use fhir_types::r4::resource::Resource;
use http::StatusCode;
use secrecy::SecretString;

use crate::outcome;

use super::error::FeedError;
use super::feed::{Event, Identifier, PatientIdentity, Unwritable};

/// The event of a feed message (the PMIR `MessageHeader` profile,
/// `MessageHeader.event[x]`).
pub(super) const FEED_EVENT: &str = "urn:ihe:iti:pmir:2019:patient-feed";

/// The event of a feed response (the PMIR `MessageHeader` response profile).
pub(super) const RESPONSE_EVENT: &str = "urn:ihe:iti:pmir:2019:patient-feed-response";

/// The `MessageDefinition` a feed message may name.
const FEED_DEFINITION: &str =
    "https://profiles.ihe.net/ITI/PMIR/MessageDefinition/IHE.PMIR.MessageDefinition";

/// The `MessageDefinition` a feed response names.
const RESPONSE_DEFINITION: &str =
    "https://profiles.ihe.net/ITI/PMIR/MessageDefinition/IHE.PMIR.MessageDefinition.Response";

/// The link type of a merged Patient (the PMIR merged Patient profile).
const REPLACED_BY: &str = "replaced-by";

/// Reads the message: its `MessageHeader.id` and its changes, in history
/// order.
pub(super) fn read(media: Option<&str>, body: &[u8]) -> Result<(String, Vec<Event>), FeedError> {
    if !outcome::fhir_json(media) {
        return Err(FeedError::NotFhirJson);
    }
    let value: Value = serde_json::from_slice(body).map_err(|error| FeedError::NotJson {
        line: error.line(),
        column: error.column(),
    })?;
    let object: &Object = value.as_object().ok_or(FeedError::NotAResource)?;
    match object.get("resourceType").and_then(Value::as_str) {
        Some("Bundle") => {}
        Some(_) => return Err(FeedError::UnexpectedResource),
        None => return Err(FeedError::NotAResource),
    }
    let bundle = Bundle::from_json(object, &mut Path::root("Bundle"))
        .map_err(|error| FeedError::Decode { kind: error.kind })?;
    if bundle.r#type.value.as_deref() != Some("message") {
        return Err(FeedError::BundleType {
            expected: "message",
        });
    }
    let found = bundle.entry.len();
    let [header_entry, history_entry] = <[BundleEntry; 2]>::try_from(bundle.entry)
        .map_err(|_entries| FeedError::EntryCount { found })?;
    let header_url = full_url(&header_entry).ok_or(FeedError::NoFullUrl { entry: 0 })?;
    let history_url = full_url(&history_entry).ok_or(FeedError::NoFullUrl { entry: 1 })?;
    let Some(Resource::MessageHeader(header)) = header_entry.resource else {
        return Err(FeedError::NoHeader);
    };
    let message_id = held_header(&header, &header_url, &history_url)?;
    let Some(Resource::Bundle(history)) = history_entry.resource else {
        return Err(FeedError::NoHistory);
    };
    if history.r#type.value.as_deref() != Some("history") {
        return Err(FeedError::BundleType {
            expected: "history",
        });
    }
    if history.entry.is_empty() {
        return Err(FeedError::EmptyHistory);
    }
    let mut changed: BTreeSet<String> = BTreeSet::new();
    let mut events = Vec::with_capacity(history.entry.len());
    for (index, entry) in history.entry.into_iter().enumerate() {
        let (event, patient) = event(index, entry)?;
        if let Some(patient) = patient
            && !changed.insert(patient)
        {
            return Err(FeedError::Duplicate { index });
        }
        events.push(event);
    }
    Ok((message_id, events))
}

/// The `MessageHeader.id`, once the header holds to the PMIR `MessageHeader`
/// profile and focuses on the entry at `history_url` (§2:3.93.4.1.2.2).
fn held_header(
    header: &MessageHeader,
    header_url: &str,
    history_url: &str,
) -> Result<String, FeedError> {
    let id = header
        .id
        .clone()
        .filter(|id| !id.is_empty())
        .ok_or(FeedError::NoMessageId)?;
    match &header.event {
        MessageHeaderEvent::Uri(uri) if uri.value.as_deref() == Some(FEED_EVENT) => {}
        _ => return Err(FeedError::Event),
    }
    if let Some(definition) = &header.definition
        && definition.value.as_deref() != Some(FEED_DEFINITION)
    {
        return Err(FeedError::Definition);
    }
    if header.destination.is_empty() {
        return Err(FeedError::NoDestination);
    }
    let [focus] = header.focus.as_slice() else {
        return Err(FeedError::Focus);
    };
    let reference = focus
        .reference
        .as_ref()
        .and_then(|reference| reference.value.as_deref())
        .ok_or(FeedError::Focus)?;
    if !resolves(reference, header_url, history_url) {
        return Err(FeedError::Focus);
    }
    Ok(id)
}

/// Whether `reference`, written in the entry at `from`, names the entry at
/// `to` (FHIR R4 bundle.html, Resolving references in Bundles): an absolute
/// reference names the entry whose `fullUrl` it is, and a relative
/// `[type]/[id]` one is appended to the base of a REST `from`.
fn resolves(reference: &str, from: &str, to: &str) -> bool {
    if reference == to {
        return true;
    }
    let Some((kind, id)) = reference.split_once('/') else {
        return false;
    };
    if kind.is_empty() || id.is_empty() || id.contains('/') {
        return false;
    }
    restful_base(from).is_some_and(|base| format!("{base}/{reference}") == to)
}

/// The `[base]` of a REST `fullUrl`, `[base]/[type]/[id]` under `http` or
/// `https` (FHIR R4 references.html, the REST URL form).
fn restful_base(full_url: &str) -> Option<&str> {
    let (rest, id) = full_url.rsplit_once('/')?;
    let (base, kind) = rest.rsplit_once('/')?;
    let restful = (base.starts_with("https://") || base.starts_with("http://"))
        && !id.is_empty()
        && kind
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_uppercase())
        && kind
            .chars()
            .all(|character| character.is_ascii_alphabetic());
    restful.then_some(base)
}

/// The entry's `fullUrl`, when it carries a non-empty one.
fn full_url(entry: &BundleEntry) -> Option<String> {
    entry
        .full_url
        .as_ref()
        .and_then(|full_url| full_url.value.clone())
        .filter(|full_url| !full_url.is_empty())
}

/// The change history entry `index` reports, and the Patient it changes when
/// the entry names one, which no other entry may change.
fn event(index: usize, entry: BundleEntry) -> Result<(Event, Option<String>), FeedError> {
    let request = entry.request.ok_or(FeedError::NoRequest { index })?;
    let response = entry.response.ok_or(FeedError::NoResponse { index })?;
    // NOTE: PMIR §2:3.93.4.1.2.3: the history holds only the changes that
    // succeeded, so an entry whose response is not a 2xx is refused.
    let succeeded = response
        .status
        .value
        .as_deref()
        .and_then(|status| status.split(' ').next())
        .and_then(|code| code.parse::<u16>().ok())
        .and_then(|code| StatusCode::from_u16(code).ok())
        .is_some_and(|status| status.is_success());
    if !succeeded {
        return Err(FeedError::Unsuccessful { index });
    }
    let named = named_patient(&request);
    match request.method.value.as_deref() {
        Some("POST") => {
            let patient = patient(index, entry.resource)?;
            if merges(&patient) {
                return Err(FeedError::Merge { index });
            }
            let key = patient.id.clone();
            Ok((Event::Created(identity(&patient)), key))
        }
        Some("PUT") => {
            let patient = patient(index, entry.resource)?;
            let key = same_patient(index, named, patient.id.clone())?;
            if merges(&patient) {
                let surviving = merged(&patient).ok_or(FeedError::Merge { index })?;
                let subsumed = identity(&patient);
                return Ok((
                    Event::Merged {
                        subsumed,
                        surviving,
                    },
                    key,
                ));
            }
            Ok((Event::Updated(identity(&patient)), key))
        }
        Some("DELETE") => {
            let named = named.ok_or(FeedError::RequestUrl { index })?;
            let identity = match entry.resource {
                None => PatientIdentity::default(),
                Some(Resource::Patient(patient)) => {
                    same_patient(index, Some(named.clone()), patient.id.clone())?;
                    identity(&patient)
                }
                Some(_) => return Err(FeedError::NotAPatient { index }),
            };
            let identity = PatientIdentity::new(
                Some(SecretString::from(named.clone())),
                identity.identifiers().to_vec(),
            );
            Ok((Event::Deleted(identity), Some(named)))
        }
        _ => Err(FeedError::Method { index }),
    }
}

/// The `Patient` a create or an update carries.
fn patient(index: usize, resource: Option<Resource>) -> Result<Box<Patient>, FeedError> {
    match resource {
        Some(Resource::Patient(patient)) => Ok(patient),
        Some(_) => Err(FeedError::NotAPatient { index }),
        None => Err(FeedError::NoResource { index }),
    }
}

/// The Patient id an update or a delete's `request.url` names,
/// `Patient/[id]` relative or under any base, with an optional
/// `/_history/[vid]`, or `None` when it names none.
fn named_patient(request: &BundleEntryRequest) -> Option<String> {
    let url = request.url.value.as_deref()?;
    let path = url.split(['?', '#']).next()?;
    let segments: Vec<&str> = path.split('/').collect();
    let at = segments.iter().rposition(|segment| *segment == "Patient")?;
    match segments.get(at.saturating_add(1)..)? {
        [id] | [id, "_history", _] if !id.is_empty() => Some((*id).to_owned()),
        _ => None,
    }
}

/// The Patient an entry changes: the one its resource carries, which must be
/// the one its `request.url` names when both do.
fn same_patient(
    index: usize,
    named: Option<String>,
    carried: Option<String>,
) -> Result<Option<String>, FeedError> {
    match (named, carried) {
        (Some(named), Some(carried)) if named != carried => Err(FeedError::RequestUrl { index }),
        (named, carried) => Ok(carried.or(named)),
    }
}

/// Whether `patient` carries a `replaced-by` link, which makes its entry a
/// merge (§2:3.93.4.1.2.4).
fn merges(patient: &Patient) -> bool {
    patient
        .link
        .iter()
        .any(|link| link.r#type.value.as_deref() == Some(REPLACED_BY))
}

/// The surviving Patient a merged `patient` names, when it holds to the
/// merged Patient profile: `active` false and exactly that one link, whose
/// `other` carries a reference.
fn merged(patient: &Patient) -> Option<SecretString> {
    let deprecated = patient
        .active
        .as_ref()
        .and_then(|active| active.value)
        .is_some_and(|active| !active);
    let [link] = patient.link.as_slice() else {
        return None;
    };
    if !deprecated || link.r#type.value.as_deref() != Some(REPLACED_BY) {
        return None;
    }
    link.other
        .reference
        .as_ref()
        .and_then(|reference| reference.value.clone())
        .filter(|reference| !reference.is_empty())
        .map(SecretString::from)
}

/// The resource id and business identifiers of `patient`, and nothing else
/// it carries.
fn identity(patient: &Patient) -> PatientIdentity {
    // NOTE: FHIR R4 Identifier.value is 0..1; an identifier with no value
    // identifies nothing, so it is legitimately absent here.
    let identifiers = patient
        .identifier
        .iter()
        .filter_map(|identifier| {
            let value = identifier.value.as_ref()?.value.clone()?;
            let system = identifier
                .system
                .as_ref()
                .and_then(|system| system.value.clone());
            Some(Identifier::new(system, SecretString::from(value)))
        })
        .collect();
    PatientIdentity::new(patient.id.clone().map(SecretString::from), identifiers)
}

/// The response to the message `request_id`, as FHIR JSON
/// (§2:3.93.4.2.2).
pub(super) fn acknowledgement(
    request_id: &str,
    response_id: &str,
    source: &str,
) -> Result<Vec<u8>, Unwritable> {
    let header = MessageHeader {
        id: Some(response_id.to_owned()),
        meta: None,
        implicit_rules: None,
        language: None,
        text: None,
        contained: Vec::new(),
        extension: Vec::new(),
        modifier_extension: Vec::new(),
        event: MessageHeaderEvent::Uri(RESPONSE_EVENT.into()),
        destination: Vec::new(),
        sender: None,
        enterer: None,
        author: None,
        source: MessageHeaderSource {
            endpoint: source.into(),
            ..MessageHeaderSource::default()
        },
        responsible: None,
        reason: None,
        response: Some(MessageHeaderResponse {
            identifier: request_id.into(),
            code: "ok".into(),
            ..MessageHeaderResponse::default()
        }),
        focus: Vec::new(),
        definition: Some(RESPONSE_DEFINITION.into()),
    };
    let bundle = Bundle {
        r#type: "message".into(),
        entry: vec![BundleEntry {
            full_url: Some(format!("urn:uuid:{response_id}").into()),
            resource: Some(Resource::MessageHeader(Box::new(header))),
            ..BundleEntry::default()
        }],
        ..Bundle::default()
    };
    serde_json::to_vec(&bundle).map_err(Unwritable)
}

/// The `OperationOutcome` refusing a message for `error`.
pub(super) fn refusal(error: &FeedError) -> Result<Vec<u8>, Unwritable> {
    let outcome = OperationOutcome {
        issue: vec![OperationOutcomeIssue {
            severity: "error".into(),
            code: error.issue().to_string().into(),
            diagnostics: Some(error.to_string().into()),
            ..OperationOutcomeIssue::default()
        }],
        ..OperationOutcome::default()
    };
    serde_json::to_vec(&outcome).map_err(Unwritable)
}

#[cfg(test)]
mod tests {
    use super::{resolves, restful_base};

    #[test]
    fn a_relative_reference_resolves_against_a_restful_full_url() {
        let header = "https://pmir.example.org/fhir/MessageHeader/m1";
        let history = "https://pmir.example.org/fhir/Bundle/h1";
        assert!(resolves("Bundle/h1", header, history));
        assert!(resolves(history, header, history), "absolute");
        assert!(!resolves("Bundle/h2", header, history), "another id");
        assert!(!resolves("Bundle/h1", "urn:uuid:1", history), "no base");
        assert!(!resolves("h1", header, history), "no type");
    }

    #[test]
    fn a_restful_full_url_has_a_type_and_an_id_under_http() {
        assert_eq!(
            Some("https://pmir.example.org/fhir"),
            restful_base("https://pmir.example.org/fhir/Patient/p1")
        );
        assert_eq!(
            None,
            restful_base("urn:uuid:7f4b3c2a-0000-4000-8000-000000000001")
        );
        assert_eq!(
            None,
            restful_base("https://pmir.example.org/fhir/patient/p1")
        );
    }
}
