// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-93 as the Patient Identity Registry receives it: `POST {base}/fhir`
//! with a PMIR feed message (PMIR 1.6.0 §2:3.93.4.1).
//!
//! The guard admits only the Source of the master domain. The message is
//! read by [`ihe_iti::pmir::server::feed`] in FHIR JSON or XML, and its
//! changes are applied in order, in one transaction with the message's BALP
//! record, to the master identity each Patient id names (§2:3.93.4.1.3;
//! [`Feeder::message`]). A message of more than [`MAX_CHANGES`] changes is
//! refused before any runs, so one transaction holds its locks briefly:
//!
//! - **Create:** the id is a UUID the person takes; a held identity is
//!   revised.
//! - **Update:** the held identity is revised.
//! - **Merge:** the deprecated identity is subsumed by the survivor its
//!   `replaced-by` link names, through the Source's duplicate resolution
//!   with `kind = source` (§2:3.93.4.1.2.4).
//! - **Delete:** the master record is removed.
//!
//! A change that would undo a merge is refused with the correlated `405`;
//! an id that is no UUID with `invalid`; an identity the Registry does not
//! hold with `404`. The response names each refused change in its
//! `MessageHeader.response.details` (§2:3.93.4.2.2), and the others are
//! applied. A failure of the store rolls back every change of the message,
//! answered with its `5xx` (§2:3.93.4.2.2 allows "an error code, 4xx or
//! 5xx"). One BALP record is written per message (§2:3.93.5.1), inside the
//! transaction, or for a message refused before any change ran, in a
//! statement of its own (#742).

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use axum::routing::{MethodFilter, MethodRouter, on};
use ferrohealth_smart::resource::Access;
use fhir_types::r4::operation_outcome::OperationOutcome;
use fhir_types::r4::patient::Patient;
use http::request::Parts;
use http::{Method, StatusCode};
use ihe_atna::balp::Outcome;
use ihe_iti::outcome::IssueType;
use ihe_iti::pmir::server::feed::{self, Applied, Change, FeedMessage};
use ihe_iti::server::answer::{Answer, Severity};
use ihe_iti::server::format::Format;
use secrecy::SecretString;
use url::Url;
use uuid::Uuid;

use crate::audit::Audit;
use crate::audit::records::{self, Call};
use crate::auth::guard::{NOT_THE_SOURCE, refusal};
use crate::auth::source::Sources;
use crate::auth::table::Route;
use crate::config::section::matching::Matching;
use crate::feed::master::{MasterChange, MasterStep, Outcomes};
use crate::feed::{Context, FeedError, Feeder};
use crate::fhir::pmir::{Refusal, answer_format, kept, refused, written};
use crate::fhir::status::Case;
use crate::fhir::wire;
use crate::store::Store;
use crate::store::record::{Channel, PersonId};
use ferrohealth_telemetry::request_log::RequestId;

/// The refusal of a change that would undo a merge (§2:3.93.4.1.3).
const UNMERGE: Applied = Applied::Refused {
    status: StatusCode::METHOD_NOT_ALLOWED,
    issue: IssueType::NotSupported,
    diagnostics: "an unmerge is not supported by this transaction",
};

/// The refusal of a Patient id that is no UUID.
const NOT_A_UUID: Applied = Applied::Refused {
    status: StatusCode::BAD_REQUEST,
    issue: IssueType::Invalid,
    diagnostics: "the Patient id is no UUID, which a master identity's id is",
};

/// The refusal of a Patient this Registry holds no master identity for.
const NOT_HELD: Applied = Applied::Refused {
    status: StatusCode::NOT_FOUND,
    issue: IssueType::NotFound,
    diagnostics: "the Patient is no master identity this Registry holds",
};

/// The refusal of a merge whose deprecated and surviving Patient are one.
const SELF_MERGE: Applied = Applied::Refused {
    status: StatusCode::BAD_REQUEST,
    issue: IssueType::Invalid,
    diagnostics: "a Patient cannot be replaced by itself",
};

/// The refusal of an identifier its domain's rule refuses.
const IDENTIFIER_REFUSED: Applied = Applied::Refused {
    status: StatusCode::UNPROCESSABLE_ENTITY,
    issue: IssueType::BusinessRule,
    diagnostics: "an identifier's value is refused by the rule of its domain",
};

/// The refusal of a change of a kind this Registry does not apply.
const UNSUPPORTED: Applied = Applied::Refused {
    status: StatusCode::BAD_REQUEST,
    issue: IssueType::NotSupported,
    diagnostics: "this change is not supported by this Registry",
};

/// The most changes one message may carry: every change of a message runs in
/// one transaction, and a larger message would hold its locks for longer (no
/// specification governs the bound: our own design).
pub const MAX_CHANGES: usize = 100;

/// What the ITI-93 route reads.
#[derive(Debug, Clone)]
pub struct Registry {
    /// The feed transaction.
    feeder: Feeder,
    /// The audit trail.
    audit: Audit,
    /// The Sources, which name the master domain.
    sources: Sources,
    /// The FHIR base, `{public_base_url}/fhir`, the Registry's endpoint.
    fhir_base: Url,
}

impl Registry {
    /// Returns the route's state over `store` with `matching`, recording to
    /// `audit`, the master domain named by `sources`, under
    /// `public_base_url`.
    #[must_use]
    pub fn new(
        store: &Store,
        matching: &Matching,
        audit: &Audit,
        sources: &Sources,
        public_base_url: &Url,
    ) -> Self {
        let base = public_base_url.as_str().trim_end_matches('/');
        let fhir_base =
            Url::parse(&format!("{base}/fhir")).unwrap_or_else(|_| public_base_url.clone());
        Self {
            feeder: Feeder::new(store.clone(), matching),
            audit: audit.clone(),
            sources: sources.clone(),
            fhir_base,
        }
    }
}

/// Returns the handler of `route` when it is the ITI-93 route, under
/// `filter`.
#[must_use]
pub fn handler(
    route: &Route,
    filter: MethodFilter,
    registry: &Arc<Registry>,
) -> Option<MethodRouter> {
    if route.method != Method::POST || route.path != "/fhir" {
        return None;
    }
    let registry = Arc::clone(registry);
    Some(on(filter, move |request: Request| {
        receive(Arc::clone(&registry), request)
    }))
}

/// `POST {base}/fhir`: one feed message (§2:3.93.4.1, §2:3.93.4.2).
async fn receive(registry: Arc<Registry>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let call = Call::of_request(&parts, &registry.fhir_base);
    let format = answer_format(&parts);
    let access = parts
        .extensions
        .get::<Access>()
        .cloned()
        .unwrap_or(Access::Open);
    let refused_with = refusal(&[registry.sources.verdict_master(&access)]);
    let (None, Some(system), Some(actor)) = (
        refused_with,
        registry.sources.master(),
        registry.sources.client(&access),
    ) else {
        let refusal = Answer::outcome(
            StatusCode::FORBIDDEN,
            Severity::Error,
            IssueType::Forbidden,
            refused_with.unwrap_or(NOT_THE_SOURCE),
        );
        return refuse(&registry, &call, &refusal, format).await;
    };
    let message = match read(&parts, body).await {
        Ok(message) => message,
        Err(refusal) => return refuse(&registry, &call, refusal.as_ref(), format).await,
    };
    let context = Context {
        actor: actor.to_owned(),
        channel: Channel::Pmir,
        request_id: parts
            .extensions
            .get::<RequestId>()
            .map(RequestId::id)
            .map(str::to_owned),
    };
    if message.changes().len() > MAX_CHANGES {
        return refuse(&registry, &call, &Case::MessageTooLarge.answer(), format).await;
    }
    let planned: Vec<Planned> = message
        .changes()
        .iter()
        .map(|change| plan(&registry, change))
        .collect();
    let steps: Vec<Option<MasterStep<'_>>> = planned
        .iter()
        .map(|planned| match planned {
            Planned::Step(step) => Some(*step),
            Planned::Settled(_) => None,
        })
        .collect();
    let records = |outcomes: &Outcomes| {
        let applied = settled_all(&planned, outcomes);
        vec![exchange(&call, &message, outcome_of(&applied))]
    };
    let committed = registry
        .feeder
        .message(&system, &steps, &context, &registry.audit, &records)
        .await;
    match committed {
        Ok(outcomes) => {
            let applied = settled_all(&planned, &outcomes);
            let response = feed::response(
                &message,
                &Uuid::new_v4().to_string(),
                registry.fhir_base.as_str(),
                &applied,
            );
            written(&response, format, None)
        }
        Err(FeedError::Unrecorded(error)) => {
            tracing::error!(error = %ferrohealth_server::run::chain(&error), "an ITI-93 message is refused: its audit record could not be kept");
            written(&refused(Refusal::Unaudited), format, None)
        }
        Err(error) => {
            let case = failed(&error);
            let exchange = exchange(&call, &message, Outcome::SeriousFailure);
            kept(&registry.audit, exchange, &case.answer(), format).await
        }
    }
}

/// What the Registry made of one change before the store: a refusal it
/// settles itself, or the write the store makes.
enum Planned<'m> {
    /// Settled without a write.
    Settled(Applied),
    /// The write.
    Step(MasterStep<'m>),
}

/// Returns the outcome of the message: success when every change applied.
fn outcome_of(applied: &[Applied]) -> Outcome {
    if applied.iter().all(|outcome| *outcome == Applied::Applied) {
        Outcome::Success
    } else {
        Outcome::MinorFailure
    }
}

/// Returns what became of each change: its own refusal, or the outcome the
/// store gave its write.
fn settled_all(planned: &[Planned<'_>], outcomes: &Outcomes) -> Vec<Applied> {
    planned
        .iter()
        .zip(outcomes)
        .map(|(planned, outcome)| match (planned, outcome) {
            (Planned::Settled(applied), _) => *applied,
            (Planned::Step(_), Some(Ok(_))) => Applied::Applied,
            (Planned::Step(_), Some(Err(error))) => refused_change(error),
            (Planned::Step(_), None) => UNSUPPORTED,
        })
        .collect()
}

/// Reads the feed message the request carries, or the answer that refuses
/// it.
async fn read(parts: &Parts, body: Body) -> Result<FeedMessage, Box<Answer<OperationOutcome>>> {
    let format = wire::body_format(&parts.headers).map_err(|case| Box::new(case.answer()))?;
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return Err(Box::new(Case::PayloadTooLarge.answer()));
    };
    FeedMessage::read(format, &bytes).map_err(|error| Box::new(error.refusal()))
}

/// Returns what the Registry makes of `change`: the write the store makes,
/// or the refusal of a change it cannot name.
fn plan<'m>(registry: &Registry, change: &'m Change) -> Planned<'m> {
    let step = match change {
        Change::Create(patient) | Change::Update(patient) => {
            let Some(id) = identity(patient) else {
                return Planned::Settled(NOT_A_UUID);
            };
            let change = if matches!(change, Change::Create(_)) {
                MasterChange::Create
            } else {
                MasterChange::Update
            };
            MasterStep::Write {
                change,
                id,
                patient,
            }
        }
        Change::Merge {
            deprecated,
            surviving,
        } => {
            let Some(id) = identity(deprecated) else {
                return Planned::Settled(NOT_A_UUID);
            };
            let survivor = match surviving_id(&registry.fhir_base, surviving) {
                Surviving::Held(survivor) => survivor,
                Surviving::NotAUuid => return Planned::Settled(NOT_A_UUID),
                Surviving::Elsewhere => return Planned::Settled(NOT_HELD),
            };
            if survivor == id {
                return Planned::Settled(SELF_MERGE);
            }
            MasterStep::Merge {
                deprecated: id,
                survivor,
                patient: deprecated,
            }
        }
        Change::Delete { id, .. } => {
            let Some(id) = uuid_of(id) else {
                return Planned::Settled(NOT_A_UUID);
            };
            MasterStep::Delete { id }
        }
        _ => return Planned::Settled(UNSUPPORTED),
    };
    Planned::Step(step)
}

/// The correlated refusal of a change the store refused on its own
/// ([`FeedError::refuses_change`]).
fn refused_change(error: &FeedError) -> Applied {
    match error {
        FeedError::Subsumed => UNMERGE,
        FeedError::Identifier { .. } => IDENTIFIER_REFUSED,
        _ => NOT_HELD,
    }
}

/// The case of a failure that ended the message's transaction.
fn failed(error: &FeedError) -> Case {
    match error {
        FeedError::Contention => Case::Contention,
        FeedError::Store(_) => {
            tracing::error!(error = %ferrohealth_server::run::chain(error), "an ITI-93 message could not be stored");
            Case::StoreUnavailable
        }
        _ => {
            tracing::error!(error = %ferrohealth_server::run::chain(error), "an ITI-93 message could not be written");
            Case::Internal
        }
    }
}

/// The master identity a Patient's id names: its UUID, written as the
/// Registry writes a person id.
fn identity(patient: &Patient) -> Option<PersonId> {
    patient.id.as_deref().and_then(uuid_of)
}

/// The person `id` names, when it is a UUID in its canonical lowercase
/// hyphenated form, so the id the Source sent is the id the Registry serves.
fn uuid_of(id: &str) -> Option<PersonId> {
    // NOTE: FHIR R4 datatypes.html#id: an id is case-sensitive, so another spelling of a UUID names another Patient.
    Uuid::try_parse(id)
        .ok()
        .filter(|uuid| uuid.to_string() == id)
        .map(PersonId::from_uuid)
}

/// What the `replaced-by` reference of a merge names.
enum Surviving {
    /// A Patient of this Registry with a UUID id.
    Held(PersonId),
    /// A Patient of this Registry whose id is no UUID.
    NotAUuid,
    /// A resource that is no Patient of this Registry.
    Elsewhere,
}

/// Reads the reference `surviving`: `Patient/{id}`, or the same under the
/// FHIR base `fhir_base` (FHIR R4 references.html, literal references).
fn surviving_id(fhir_base: &Url, surviving: &str) -> Surviving {
    let base = fhir_base.as_str().trim_end_matches('/');
    let relative = surviving
        .strip_prefix(base)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(surviving);
    let Some(id) = relative.strip_prefix("Patient/") else {
        return Surviving::Elsewhere;
    };
    match uuid_of(id) {
        Some(survivor) => Surviving::Held(survivor),
        None if id.contains('/') => Surviving::Elsewhere,
        None => Surviving::NotAUuid,
    }
}

/// The record of `message`, ended in `outcome`, naming each Patient its
/// changes name.
fn exchange(call: &Call, message: &FeedMessage, outcome: Outcome) -> ihe_atna::balp::Exchange {
    let reference = |id: &str| SecretString::from(format!("Patient/{id}"));
    let mut patients = Vec::new();
    for change in message.changes() {
        match change {
            Change::Create(patient) | Change::Update(patient) => {
                patients.extend(patient.id.as_deref().map(reference));
            }
            Change::Merge {
                deprecated,
                surviving,
            } => {
                patients.extend(deprecated.id.as_deref().map(reference));
                patients.push(SecretString::from(surviving.clone()));
            }
            Change::Delete { id, .. } => patients.push(reference(id)),
            _ => {}
        }
    }
    records::pmir_feed(call, patients, Some(message.id()), outcome)
}

/// Records a message refused before any change ran, then sends `refusal`.
async fn refuse(
    registry: &Registry,
    call: &Call,
    refusal: &Answer<OperationOutcome>,
    format: Format,
) -> Response {
    let exchange = records::pmir_feed(call, Vec::new(), None, Outcome::MinorFailure);
    kept(&registry.audit, exchange, refusal, format).await
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{Surviving, surviving_id, uuid_of};

    const ID: &str = "3f0c2a5e-0000-4000-8000-000000000001";

    #[test]
    fn only_a_canonical_uuid_names_a_master_identity() {
        assert!(uuid_of(ID).is_some());
        assert!(uuid_of(&ID.to_uppercase()).is_none(), "another spelling");
        assert!(uuid_of(&ID.replace('-', "")).is_none(), "the simple form");
        assert!(uuid_of("ex-patient-merge").is_none());
    }

    #[test]
    fn a_survivor_is_a_patient_of_this_registry_by_a_relative_or_absolute_reference() {
        let base = Url::parse("https://pix.example.org/fhir").expect("a URL");
        let held = |reference: &str| matches!(surviving_id(&base, reference), Surviving::Held(_));
        assert!(held(&format!("Patient/{ID}")));
        assert!(held(&format!("https://pix.example.org/fhir/Patient/{ID}")));
        assert!(matches!(
            surviving_id(&base, "Patient/ex-patient-merged"),
            Surviving::NotAUuid
        ));
        assert!(matches!(
            surviving_id(
                &base,
                &format!("https://other.example.org/fhir/Patient/{ID}")
            ),
            Surviving::Elsewhere
        ));
        assert!(matches!(
            surviving_id(&base, &format!("Practitioner/{ID}")),
            Surviving::Elsewhere
        ));
    }
}
