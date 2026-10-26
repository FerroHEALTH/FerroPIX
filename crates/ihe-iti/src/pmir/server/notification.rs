// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-93 as the Patient Identity Registry sends it: the feed message for
//! the changes to Patient Master Identities a subscription matches
//! (§2:3.93.4.1.1, §2:3.93.4.1.2).

use std::fmt;

use fhir_types::r4::bundle::{Bundle, BundleEntry, BundleEntryRequest, BundleEntryResponse};
use fhir_types::r4::message_header::{
    MessageHeader, MessageHeaderDestination, MessageHeaderEvent, MessageHeaderSource,
};
use fhir_types::r4::patient::Patient;
use fhir_types::r4::reference::Reference;
use fhir_types::r4::resource::Resource;
use url::Url;

use super::super::message::FEED_EVENT;

/// The `MessageDefinition` a feed message names.
const FEED_DEFINITION: &str =
    "https://profiles.ihe.net/ITI/PMIR/MessageDefinition/IHE.PMIR.MessageDefinition";

/// One change the Registry made, as the feed reports it.
///
/// `Debug` shows the kind of change, never a Patient.
#[derive(Clone)]
#[non_exhaustive]
pub enum Notice {
    /// A Patient Master Identity was created.
    Created(Box<Patient>),
    /// A Patient Master Identity was updated.
    Updated(Box<Patient>),
    /// A Patient Master Identity was deleted, by its id.
    Deleted(String),
    /// A Patient Master Identity was merged: the deprecated Patient,
    /// `active` false with its one `replaced-by` link to the survivor, sent
    /// as an update (§2:3.93.4.1.2.4).
    Merged(Box<Patient>),
}

impl fmt::Debug for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Created(_) => "Created",
            Self::Updated(_) => "Updated",
            Self::Deleted(_) => "Deleted",
            Self::Merged(_) => "Merged",
        })
    }
}

/// The addresses of one message: its ids, the Registry's base, which is the
/// Master Patient Identification Domain (§2:3.93.4.1.3), and the
/// subscriber's channel endpoint.
#[derive(Debug, Clone)]
pub struct Addressing<'a> {
    /// The `MessageHeader.id`, which the subscriber's response names.
    pub message_id: &'a str,
    /// The id of the history Bundle.
    pub history_id: &'a str,
    /// The Registry's FHIR base.
    pub registry: &'a Url,
    /// The subscription's channel endpoint.
    pub destination: &'a Url,
}

/// The ITI-93 message reporting `notices`, in order, under `addressing`.
///
/// The message holds to the PMIR profiles: a `message` Bundle whose first
/// entry is the `MessageHeader` with the feed event, the subscriber as its
/// destination, the Registry as its source and its one focus on the second
/// entry, the `history` Bundle (§2:3.93.4.1.2.1 to §2:3.93.4.1.2.3). Each
/// history entry carries its request and a successful response; a Patient's
/// `fullUrl` is its `RESTful` URL under the Registry's base.
#[must_use]
pub fn message(notices: &[Notice], addressing: &Addressing<'_>) -> Bundle {
    let base = addressing.registry.as_str().trim_end_matches('/');
    let history_url = format!("urn:uuid:{}", addressing.history_id);
    let entries = notices
        .iter()
        .map(|notice| history_entry(notice, base))
        .collect();
    let header = MessageHeader {
        id: Some(addressing.message_id.to_owned()),
        meta: None,
        implicit_rules: None,
        language: None,
        text: None,
        contained: Vec::new(),
        extension: Vec::new(),
        modifier_extension: Vec::new(),
        event: MessageHeaderEvent::Uri(FEED_EVENT.into()),
        destination: vec![MessageHeaderDestination {
            endpoint: addressing.destination.as_str().into(),
            ..MessageHeaderDestination::default()
        }],
        sender: None,
        enterer: None,
        author: None,
        source: MessageHeaderSource {
            endpoint: base.into(),
            ..MessageHeaderSource::default()
        },
        responsible: None,
        reason: None,
        response: None,
        focus: vec![Reference {
            reference: Some(history_url.as_str().into()),
            ..Reference::default()
        }],
        definition: Some(FEED_DEFINITION.into()),
    };
    Bundle {
        r#type: "message".into(),
        entry: vec![
            BundleEntry {
                full_url: Some(format!("urn:uuid:{}", addressing.message_id).into()),
                resource: Some(Resource::MessageHeader(Box::new(header))),
                ..BundleEntry::default()
            },
            BundleEntry {
                full_url: Some(history_url.into()),
                resource: Some(Resource::Bundle(Box::new(Bundle {
                    r#type: "history".into(),
                    entry: entries,
                    ..Bundle::default()
                }))),
                ..BundleEntry::default()
            },
        ],
        ..Bundle::default()
    }
}

/// The history entry of one notice.
fn history_entry(notice: &Notice, base: &str) -> BundleEntry {
    let (method, url, status, patient, id) = match notice {
        Notice::Created(patient) => (
            "POST",
            String::from("Patient"),
            "201",
            Some(patient.clone()),
            patient.id.clone(),
        ),
        Notice::Updated(patient) | Notice::Merged(patient) => {
            let id = patient.id.clone().unwrap_or_default();
            (
                "PUT",
                format!("Patient/{id}"),
                "200",
                Some(patient.clone()),
                Some(id),
            )
        }
        Notice::Deleted(id) => (
            "DELETE",
            format!("Patient/{id}"),
            "204",
            None,
            Some(id.clone()),
        ),
    };
    BundleEntry {
        full_url: id.map(|id| format!("{base}/Patient/{id}").into()),
        resource: patient.map(Resource::Patient),
        request: Some(BundleEntryRequest {
            method: method.into(),
            url: url.as_str().into(),
            ..BundleEntryRequest::default()
        }),
        response: Some(BundleEntryResponse {
            status: status.into(),
            ..BundleEntryResponse::default()
        }),
        ..BundleEntry::default()
    }
}
