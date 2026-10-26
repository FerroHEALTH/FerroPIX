// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-93 as the Patient Identity Registry receives it: the feed message
//! read with its Patients, and the response written (§2:3.93.4.1.2,
//! §2:3.93.4.2.2).

use std::fmt;

use fhir_types::r4::bundle::{Bundle, BundleEntry};
use fhir_types::r4::message_header::{
    MessageHeader, MessageHeaderEvent, MessageHeaderResponse, MessageHeaderSource,
};
use fhir_types::r4::operation_outcome::{OperationOutcome, OperationOutcomeIssue};
use fhir_types::r4::patient::Patient;
use fhir_types::r4::reference::Reference;
use fhir_types::r4::resource::Resource;
use http::StatusCode;

use super::super::error::FeedError;
use super::super::message::{self, RESPONSE_EVENT};
use crate::outcome::IssueType;
use crate::server::answer::Answer;
use crate::server::format::{BodyError, Format};

/// The `MessageDefinition` a feed response names.
const RESPONSE_DEFINITION: &str =
    "https://profiles.ihe.net/ITI/PMIR/MessageDefinition/IHE.PMIR.MessageDefinition.Response";

/// The local id of the `OperationOutcome` a refusing response contains.
const OUTCOME_ID: &str = "outcome";

/// One change of a feed message, with the Patient it carries.
///
/// `Debug` shows the kind of change, never a Patient.
#[derive(Clone)]
#[non_exhaustive]
pub enum Change {
    /// A create (`POST`): the Patient to create in the master domain.
    Create(Box<Patient>),
    /// An update (`PUT`) with no `replaced-by` link: the Patient as it now
    /// stands.
    Update(Box<Patient>),
    /// A delete (`DELETE`): the id the `request.url` names, and the Patient
    /// when the entry carries it.
    Delete {
        /// The id of the deleted Patient.
        id: String,
        /// The Patient, when the entry carries it.
        patient: Option<Box<Patient>>,
    },
    /// A merge: the deprecated Patient, `active` false with one
    /// `replaced-by` link (§2:3.93.4.1.2.4), and the reference to the
    /// surviving Patient that link names.
    Merge {
        /// The deprecated Patient.
        deprecated: Box<Patient>,
        /// The reference to the surviving Patient, as written.
        surviving: String,
    },
}

impl fmt::Debug for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Create(_) => "Create",
            Self::Update(_) => "Update",
            Self::Delete { .. } => "Delete",
            Self::Merge { .. } => "Merge",
        })
    }
}

impl From<message::Change> for Change {
    fn from(change: message::Change) -> Self {
        match change {
            message::Change::Created(patient) => Self::Create(patient),
            message::Change::Updated(patient) => Self::Update(patient),
            message::Change::Deleted { id, patient } => Self::Delete { id, patient },
            message::Change::Merged {
                deprecated,
                surviving,
            } => Self::Merge {
                deprecated,
                surviving,
            },
        }
    }
}

/// One ITI-93 feed message, read and held to the PMIR profiles.
#[derive(Debug, Clone)]
pub struct FeedMessage {
    id: String,
    source: String,
    changes: Vec<Change>,
}

impl FeedMessage {
    /// Reads the feed message `body`, in `format`.
    ///
    /// The message is a `Bundle` of type `message` with exactly two entries:
    /// a `MessageHeader` whose event is `urn:ihe:iti:pmir:2019:patient-feed`,
    /// with a destination and one focus on the second entry, and a `history`
    /// Bundle of unique, successful Patient creates, updates, merges and
    /// deletes (§2:3.93.4.1.2.1 to §2:3.93.4.1.2.3). A Patient with a
    /// `replaced-by` link is a merge, sent as an update, and holds to the
    /// merged Patient profile (§2:3.93.4.1.2.4).
    ///
    /// # Errors
    /// A [`ReadError`] for a body that is no FHIR resource in `format`, or a
    /// message that departs from any of those rules; its
    /// [`refusal`](ReadError::refusal) is the answer to send.
    pub fn read(format: Format, body: &[u8]) -> Result<Self, ReadError> {
        let object = format.decode(body).map_err(ReadError::Body)?;
        let read = message::read_bundle(&object).map_err(ReadError::Message)?;
        Ok(Self {
            id: read.id,
            source: read.source,
            changes: read.changes.into_iter().map(Change::from).collect(),
        })
    }

    /// The `MessageHeader.id`, which the response names.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The `MessageHeader.source.endpoint` of the Source that sent it.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The changes, in the order of the history Bundle.
    #[must_use]
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }
}

/// Why a feed message is refused.
///
/// No variant carries text from the body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ReadError {
    /// The body is no FHIR R4 resource in the format it was sent in.
    #[error(transparent)]
    Body(BodyError),
    /// The message departs from the PMIR profiles.
    #[error(transparent)]
    Message(FeedError),
}

impl ReadError {
    /// The answer that refuses the message: `400` with an `OperationOutcome`
    /// naming the rule it broke (FHIR R4 messaging.html, the response to a
    /// message that cannot be processed).
    #[must_use]
    pub fn refusal(&self) -> Answer<OperationOutcome> {
        let code = match self {
            Self::Body(_) => IssueType::Structure,
            Self::Message(error) => error.issue(),
        };
        Answer::new(
            StatusCode::BAD_REQUEST,
            OperationOutcome {
                issue: vec![OperationOutcomeIssue {
                    severity: "error".into(),
                    code: code.to_string().into(),
                    diagnostics: Some(self.to_string().into()),
                    ..OperationOutcomeIssue::default()
                }],
                ..OperationOutcome::default()
            },
        )
    }
}

/// What the Registry did with one change of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Applied {
    /// The change was applied.
    Applied,
    /// The change was refused: an unmerge is `405` (§2:3.93.4.1.3), and
    /// another refusal names its own status and issue type.
    Refused {
        /// The status the refusal correlates with the entry.
        status: StatusCode,
        /// The issue type of the refusal.
        issue: IssueType,
        /// The Registry's own words; they never quote the message.
        diagnostics: &'static str,
    },
}

/// The response to `message` from the Registry at `registry`, with
/// `response_id` its own `MessageHeader.id` and `applied` what became of each
/// change, in the order of [`FeedMessage::changes`] (§2:3.93.4.2.2).
///
/// The response is a `Bundle` of type `message` with one `MessageHeader`
/// that holds to the PMIR response profile: the feed response event, no
/// destination, the response naming the message (§2:3.93.4.2.2). When every
/// change was applied its code is `ok`. When one was refused it is
/// `fatal-error`, and its `details` name a contained `OperationOutcome` with
/// one issue per refused change whose `expression` is the change's history
/// entry and whose `diagnostics` carry the correlated status.
#[must_use]
pub fn response(
    message: &FeedMessage,
    response_id: &str,
    registry: &str,
    applied: &[Applied],
) -> Answer<Bundle> {
    let issues: Vec<OperationOutcomeIssue> = applied
        .iter()
        .enumerate()
        .filter_map(|(index, outcome)| match outcome {
            Applied::Applied => None,
            Applied::Refused {
                status,
                issue,
                diagnostics,
            } => Some(OperationOutcomeIssue {
                severity: "error".into(),
                code: issue.to_string().into(),
                diagnostics: Some(format!("{}: {diagnostics}", status.as_u16()).into()),
                expression: vec![format!("Bundle.entry[1].resource.entry[{index}]").into()],
                ..OperationOutcomeIssue::default()
            }),
        })
        .collect();
    // NOTE: PMIR §2:3.93.4.1.3 asks for a "correlated entry.response.code" of 405 but §2:3.93.4.2.2 allows one MessageHeader entry; FHIR R4 MessageHeader.response.details carries it.
    let (code, details, contained) = if issues.is_empty() {
        ("ok", None, Vec::new())
    } else {
        let outcome = OperationOutcome {
            id: Some(OUTCOME_ID.to_owned()),
            issue: issues,
            ..OperationOutcome::default()
        };
        (
            "fatal-error",
            Some(Reference {
                reference: Some(format!("#{OUTCOME_ID}").as_str().into()),
                ..Reference::default()
            }),
            vec![Resource::OperationOutcome(Box::new(outcome))],
        )
    };
    let header = MessageHeader {
        id: Some(response_id.to_owned()),
        meta: None,
        implicit_rules: None,
        language: None,
        text: None,
        contained,
        extension: Vec::new(),
        modifier_extension: Vec::new(),
        event: MessageHeaderEvent::Uri(RESPONSE_EVENT.into()),
        destination: Vec::new(),
        sender: None,
        enterer: None,
        author: None,
        source: MessageHeaderSource {
            endpoint: registry.into(),
            ..MessageHeaderSource::default()
        },
        responsible: None,
        reason: None,
        response: Some(MessageHeaderResponse {
            identifier: message.id.as_str().into(),
            code: code.into(),
            details,
            ..MessageHeaderResponse::default()
        }),
        focus: Vec::new(),
        definition: Some(RESPONSE_DEFINITION.into()),
    };
    Answer::new(
        StatusCode::OK,
        Bundle {
            r#type: "message".into(),
            entry: vec![BundleEntry {
                full_url: Some(format!("urn:uuid:{response_id}").into()),
                resource: Some(Resource::MessageHeader(Box::new(header))),
                ..BundleEntry::default()
            }],
            ..Bundle::default()
        },
    )
}
