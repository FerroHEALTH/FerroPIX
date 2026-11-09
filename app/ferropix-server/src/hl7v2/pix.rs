// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-9, the PIX Query, as the Manager answers it.
//!
//! QBP^Q23 names one identifier in QPD-3 and, in QPD-4, the domains whose
//! identifiers it wants (ITI TF-2 §3.9.4.1.2.2). The answer is RSP^K23 from
//! the cross-reference ITI-83 reads ([`xref::lookup`]), with the same policy
//! for a merged identifier (the survivor's identifiers) and a removed one
//! (not found), and the six cases of §3.9.4.2.2.6: `OK` with one PID whose
//! PID-3 holds every identifier but the one asked, `NF` with none, and `AE`
//! with the ERR the case names for an unknown identifier (Case 3), an
//! unknown domain (Case 4) and an unknown requested domain (Case 5). The PID
//! carries no name (§3.9.4.2.2.5). An identifier carried by two persons'
//! records is answered `AE` with code `207`, as ITI-83 answers it `409`
//! (no specification governs this: our own design). The record of each
//! query is stored before the answer leaves (§3.9.5.1.2).

use ferrobridge_hl7v2::ack::{Code, Error, Header};
use ferrobridge_hl7v2::parse::{Field, Message};
use ihe_atna::balp::Outcome;
use secrecy::SecretString;

use crate::audit::records::{self, Call, Identifier, V2Query};
use crate::config::section::hl7v2::authority::Hd;
use crate::hl7v2::{Answer, Face, Response, respond};
use crate::hl7v2::{fault, fault_at, location};
use crate::store::identifier::Rule;
use crate::store::record;
use crate::store::xref::{self, Lookup};
use crate::v2::pid::{self, Shown};
use crate::v2::stamped::Stamped;
use ferrobridge_hl7v2::parse::ErrorCode;
use ferrobridge_hl7v2::parse::profile::Profile;

/// What a query settled.
struct Settled {
    /// MSA-1.
    code: Code,
    /// QAK-2, a table 0208 code.
    status: &'static str,
    /// The ERR segments.
    faults: Vec<Error>,
    /// The identifiers PID-3 carries, `None` for no PID.
    identifiers: Option<Vec<(String, String)>>,
    /// The person found, `Patient/{id}`.
    persons: Vec<String>,
}

impl Settled {
    /// An `AE` with `faults`.
    fn error(faults: Vec<Error>) -> Self {
        Self {
            code: Code::Error,
            status: "AE",
            faults,
            identifiers: None,
            persons: Vec::new(),
        }
    }

    /// An `AR` for a reason unrelated to the query.
    fn rejected(text: &'static str) -> Self {
        Self {
            code: Code::Reject,
            status: "AR",
            faults: vec![fault(ErrorCode::Application, text)],
            identifiers: None,
            persons: Vec::new(),
        }
    }
}

/// Answers one QBP^Q23 of `sender`, whose text is `text`.
pub(super) async fn query(
    face: &Face,
    message: &Message,
    header: &Header,
    call: &Call,
    text: &str,
    stamped: &Stamped,
) -> Answer {
    let qpd = message
        .segments()
        .iter()
        .find(|segment| segment.id() == "QPD");
    let source = qpd
        .and_then(|qpd| qpd.field(3))
        .and_then(|field| field.repetitions().first());
    let value = source
        .and_then(|cx| cx.component(1))
        .and_then(|component| component.subcomponent(1))
        .map(str::to_owned);
    let system = face
        .authorities
        .system(&Hd::of(source.and_then(|cx| cx.component(4))));
    let wanted = qpd.and_then(|qpd| qpd.field(4));
    let settled = match face.audit.reserve() {
        Err(error) => {
            tracing::error!(error = %ferrohealth_server::run::chain(&error), "an ITI-9 query is refused: its audit record has no room");
            return answer(
                header,
                message,
                &Settled::rejected(
                    "the audit record could not be stored, so the query was not answered",
                ),
                face,
                stamped,
            );
        }
        Ok(place) => {
            let settled = settle(face, system.as_deref(), value.as_deref(), wanted).await;
            let identifiers: Vec<Identifier> = system
                .iter()
                .zip(value.iter())
                .map(|(system, value)| Identifier {
                    system: system.clone(),
                    value: SecretString::from(value.clone()),
                })
                .collect();
            let outcome = if settled.code == Code::Accept {
                Outcome::Success
            } else {
                Outcome::MinorFailure
            };
            let exchange = records::v2_query(
                call,
                V2Query::Pix,
                SecretString::from(text.to_owned()),
                &identifiers,
                &settled.persons,
                outcome,
            );
            if let Err(error) = place.record(&exchange).await {
                tracing::error!(error = %ferrohealth_server::run::chain(&error), "an ITI-9 answer is withheld: its audit record could not be stored");
                Settled::rejected(
                    "the audit record could not be stored, so the query was not answered",
                )
            } else {
                settled
            }
        }
    };
    answer(header, message, &settled, face, stamped)
}

/// Reads the cross-reference of `value` in `system` and settles the case.
async fn settle(
    face: &Face,
    system: Option<&str>,
    value: Option<&str>,
    wanted: Option<&Field>,
) -> Settled {
    let unknown_domain = || {
        Settled::error(vec![fault_at(
            location("QPD", 1, 3, Some(1), Some(4)),
            ErrorCode::UnknownKeyIdentifier,
            "the assigning authority of QPD-3 is not recognized",
        )])
    };
    let Some(system) = system else {
        return unknown_domain();
    };
    let Some(value) = value else {
        return Settled::error(vec![fault_at(
            location("QPD", 1, 3, Some(1), Some(1)),
            ErrorCode::RequiredFieldMissing,
            "QPD-3 carries no identifier",
        )]);
    };
    let mut connection = match face.store.pool().acquire().await {
        Ok(connection) => connection,
        Err(error) => {
            tracing::error!(%error, "the store could not be reached for an ITI-9 query");
            return Settled::rejected("the store could not be read");
        }
    };
    let found = match xref::lookup(&mut connection, face.store.sealer(), system, value).await {
        Ok(found) => found,
        Err(error) => {
            tracing::error!(error = %ferrohealth_server::run::chain(&error), "an ITI-9 query could not be read");
            return Settled::rejected("the store could not be read");
        }
    };
    if found == Lookup::UnknownDomain {
        return unknown_domain();
    }
    let targets = match targets(face, &mut connection, wanted).await {
        Ok(targets) => targets,
        Err(settled) => return settled,
    };
    let cross_reference = match found {
        Lookup::Found(cross_reference) => cross_reference,
        Lookup::Ambiguous => {
            return Settled::error(vec![fault_at(
                location("QPD", 1, 3, Some(1), Some(1)),
                ErrorCode::Application,
                "the identifier is carried in the records of more than one person",
            )]);
        }
        Lookup::UnknownDomain | Lookup::NotFound => {
            return Settled::error(vec![fault_at(
                location("QPD", 1, 3, Some(1), Some(1)),
                ErrorCode::UnknownKeyIdentifier,
                "the identifier of QPD-3 is not recognized",
            )]);
        }
    };
    // NOTE: ITI TF-2 §3.9.4.2.2.6 Case 1, the identifier asked is not returned;
    // it is compared as its domain's rule normalises it, as the store holds it.
    let asked = Rule::of(system).normalise(value).ok();
    let identifiers: Vec<(String, String)> = cross_reference
        .identifiers
        .iter()
        .filter(|(held, held_value)| {
            !(held == system && asked.as_ref().is_some_and(|asked| asked == held_value))
        })
        .filter(|(held, _)| targets.is_empty() || targets.contains(held))
        .map(|(held, held_value)| (held.clone(), held_value.expose().to_owned()))
        .collect();
    let persons = vec![format!("Patient/{}", cross_reference.person.uuid())];
    if identifiers.is_empty() {
        return Settled {
            code: Code::Accept,
            status: "NF",
            faults: Vec::new(),
            identifiers: None,
            persons,
        };
    }
    Settled {
        code: Code::Accept,
        status: "OK",
        faults: Vec::new(),
        identifiers: Some(identifiers),
        persons,
    }
}

/// Reads QPD-4, the domains whose identifiers are returned, refusing every
/// one the store does not hold or retired (ITI TF-2 §3.9.4.2.2.6, Case 5).
async fn targets(
    face: &Face,
    connection: &mut sqlx::PgConnection,
    wanted: Option<&Field>,
) -> Result<Vec<String>, Settled> {
    let domains = record::domains(&mut *connection).await.map_err(|error| {
        tracing::error!(error = %ferrohealth_server::run::chain(&error), "the domains could not be read for an ITI-9 query");
        Settled::rejected("the store could not be read")
    })?;
    let mut targets = Vec::new();
    let mut faults = Vec::new();
    let repetitions = wanted.map(Field::repetitions).unwrap_or_default();
    for (index, repetition) in repetitions.iter().enumerate() {
        if !repetition.is_valued() {
            continue;
        }
        let target = face.authorities.system(&Hd::of(repetition.component(4)));
        match target.filter(|target| domains.iter().any(|d| d.system == *target && !d.retired)) {
            Some(target) => targets.push(target),
            None => faults.push(fault_at(
                location("QPD", 1, 4, Some(index.saturating_add(1)), None),
                ErrorCode::UnknownKeyIdentifier,
                "a domain of QPD-4 is not recognized",
            )),
        }
    }
    if faults.is_empty() {
        Ok(targets)
    } else {
        Err(Settled::error(faults))
    }
}

/// Writes the RSP^K23 of `settled`.
fn answer(
    header: &Header,
    message: &Message,
    settled: &Settled,
    face: &Face,
    stamped: &Stamped,
) -> Answer {
    let body = settled
        .identifiers
        .iter()
        .map(|identifiers| pid::segment(&face.authorities, 1, identifiers, Shown::Pseudonymous))
        .collect();
    respond(
        header,
        message,
        Response {
            message_type: ["RSP", "K23", "RSP_K23"],
            code: settled.code,
            faults: &settled.faults,
            status: settled.status,
            body,
            profile: &Profile::BASE,
        },
        stamped,
    )
}
