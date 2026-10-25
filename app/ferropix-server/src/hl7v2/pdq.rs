// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-21, the Patient Demographics Query, as the Supplier answers it.
//!
//! QBP^Q22 names its criteria as QPD-3 parameters (ITI TF-2
//! §3.21.4.1.2.3) and the domains whose identifiers it wants in QPD-8
//! (§3.21.4.1.2.4). Each parameter of Table 3.21-3 and Table 3.21-4 is read
//! as the ITI-78 search parameter of the same element, and the search ITI-78
//! runs answers it ([`Supplier::search`](crate::pdqm::Supplier::search)), so both faces find the same
//! persons. The answer is RSP^K22 with one PID per person (§3.21.4.2.2.5)
//! and the three cases of §3.21.4.2.2.8: `OK` or `NF`, a PID left out when
//! the domain filter leaves its PID-3 empty (Case 2), and `AE` with an ERR
//! per unknown domain of QPD-8 (Case 3).
//!
//! RCP-2 asks for increments of that many records (§3.21.4.1.2.5.2); an
//! answer that holds fewer than match ends with a DSC whose pointer the
//! Consumer echoes for the next increment (§3.21.4.2.2.7), kept by
//! [`Cursors`](crate::hl7v2::cursor::Cursors) and given up by QCN^J01
//! (§3.21.4.3). A parameter the Supplier does not search on, and a pointer
//! unknown or past its bound, are refused `AE`: no specification fixes the
//! answer to either, so the code and location are our own design. The
//! record of each query is stored before the answer leaves (§3.21.5.1.2).

use std::collections::BTreeMap;

use ferrobridge_hl7v2::ack::Code;
use ferrobridge_hl7v2::parse::{Message, Segment};
use fhir_types::codec::{Json as _, Object, Value};
use fhir_types::r4::resource::Resource;
use ihe_atna::balp::Outcome;
use ihe_iti::pdqm::server::search::PatientSearch;
use secrecy::{ExposeSecret as _, SecretString};
use uuid::Uuid;

use crate::audit::records::{self, Call, V2Query};
use crate::config::section::hl7v2::Sender;
use crate::hl7v2::authority::Hd;
use crate::hl7v2::cursor::Place;
use crate::hl7v2::feed::{fhir_escape, query_part, url_encoded};
use crate::hl7v2::pid::{self, Shown};
use crate::hl7v2::stamped::Stamped;
use crate::hl7v2::write::{Fault, Writer, location};
use crate::hl7v2::{Answer, Face, acknowledge, sender_key};
use crate::pdqm::{MAX_PAGE, Searched};
use crate::store::record;
use ferrobridge_hl7v2::parse::ErrorCode;

/// One person of an answer: its id and its `Patient`'s JSON.
type Found = (Uuid, Object);

/// One person of an answer with the identifiers its PID-3 carries.
type Row = (Uuid, Object, Vec<(String, String)>);

/// What a query settled.
struct Settled {
    /// MSA-1.
    code: Code,
    /// QAK-2, a table 0208 code.
    status: &'static str,
    /// The ERR segments.
    faults: Vec<Fault>,
    /// The persons of the increment, each with the identifiers its PID-3
    /// carries.
    found: Vec<Row>,
    /// The continuation pointer, when records follow.
    pointer: Option<String>,
}

impl Settled {
    /// An `AE` with `faults`.
    fn error(faults: Vec<Fault>) -> Self {
        Self {
            code: Code::Error,
            status: "AE",
            faults,
            found: Vec::new(),
            pointer: None,
        }
    }

    /// An `AR` for a reason unrelated to the query.
    fn rejected(text: &'static str) -> Self {
        Self {
            code: Code::Reject,
            status: "AR",
            faults: vec![Fault::anywhere(ErrorCode::Application, text)],
            found: Vec::new(),
            pointer: None,
        }
    }
}

/// Answers one QBP^Q22 of `sender`, whose text is `text`.
pub(super) async fn query(
    face: &Face,
    message: &Message,
    sender: &Sender,
    call: &Call,
    text: &str,
    stamped: &Stamped,
) -> Answer {
    let place = match face.audit.reserve() {
        Ok(place) => place,
        Err(error) => {
            tracing::error!(error = %crate::chain(&error), "an ITI-21 query is refused: its audit record has no room");
            return answer(
                message,
                &Settled::rejected(
                    "the audit record could not be stored, so the query was not answered",
                ),
                face,
                stamped,
            );
        }
    };
    let settled = settle(face, message, sender).await;
    let persons: Vec<String> = settled
        .found
        .iter()
        .map(|(id, _, _)| format!("Patient/{id}"))
        .collect();
    let outcome = if settled.code == Code::Accept {
        Outcome::Success
    } else {
        Outcome::MinorFailure
    };
    let exchange = records::v2_query(
        call,
        V2Query::Pdq,
        SecretString::from(text.to_owned()),
        &[],
        &persons,
        outcome,
    );
    if let Err(error) = place.record(&exchange).await {
        tracing::error!(error = %crate::chain(&error), "an ITI-21 answer is withheld: its audit record could not be stored");
        return answer(
            message,
            &Settled::rejected(
                "the audit record could not be stored, so the query was not answered",
            ),
            face,
            stamped,
        );
    }
    answer(message, &settled, face, stamped)
}

/// Gives up the continuation pointers of the query QCN^J01 names in QID-1,
/// acknowledged with an `ACK` (ITI TF-2 §3.21.4.3.2).
pub(super) fn cancel(face: &Face, message: &Message, sender: &Sender, stamped: &Stamped) -> Answer {
    let tag = segment(message, "QID")
        .and_then(|qid| qid.text(1))
        .unwrap_or_default();
    let removed = face.cursors.cancel(&sender_key(sender), tag);
    tracing::info!(removed, "a query is cancelled");
    acknowledge(message, Code::Accept, &[], stamped)
}

/// Reads the query, runs the search and settles the case.
async fn settle(face: &Face, message: &Message, sender: &Sender) -> Settled {
    let key = sender_key(sender);
    let tag = segment(message, "QPD")
        .and_then(|qpd| qpd.text(2))
        .unwrap_or_default()
        .to_owned();
    let increment = match increment(message) {
        Ok(increment) => increment,
        Err(fault) => return Settled::error(vec![fault]),
    };
    let pointer = segment(message, "DSC").and_then(|dsc| dsc.text(1));
    let (search, filtered, after, increment) = match pointer {
        Some(pointer) => match face.cursors.take(pointer, &key, &tag) {
            Some(place) => (
                place.search,
                place.filtered,
                Some(place.after),
                Some(place.increment),
            ),
            None => {
                return Settled::error(vec![Fault::at(
                    location("DSC", 1, 1, None, None),
                    ErrorCode::UnknownKeyIdentifier,
                    "the continuation pointer is unknown or past its bound",
                )]);
            }
        },
        None => match criteria(face, message).await {
            Ok(Some(criteria)) => (criteria.search, criteria.filtered, None, increment),
            Ok(None) => {
                return Settled {
                    code: Code::Accept,
                    status: "NF",
                    faults: Vec::new(),
                    found: Vec::new(),
                    pointer: None,
                };
            }
            Err(settled) => return settled,
        },
    };
    let (found, more) = match run(face, &search, after, increment).await {
        Ok(run) => run,
        Err(settled) => return settled,
    };
    let pointer = match (more, increment) {
        (Some(last), Some(increment)) => Some(face.cursors.keep(Place {
            sender: key,
            tag,
            search,
            filtered,
            after: last,
            increment,
        })),
        _ => None,
    };
    let found: Vec<Row> = found
        .into_iter()
        .map(|(id, patient)| {
            let identifiers = identifiers(&patient);
            (id, patient, identifiers)
        })
        .filter(|(_, _, identifiers)| !filtered || !identifiers.is_empty())
        .collect();
    Settled {
        code: Code::Accept,
        status: if found.is_empty() { "NF" } else { "OK" },
        faults: Vec::new(),
        found,
        pointer,
    }
}

/// Reads RCP-2, the records an increment holds, `None` when it names none.
fn increment(message: &Message) -> Result<Option<u32>, Fault> {
    let Some(quantity) = segment(message, "RCP")
        .and_then(|rcp| rcp.field(2))
        .and_then(|field| field.repetitions().first())
    else {
        return Ok(None);
    };
    let amount = quantity.component(1).and_then(|component| component.text());
    let units = quantity.component(2).and_then(|component| component.text());
    let Some(amount) = amount else {
        return Ok(None);
    };
    let refused = || {
        Fault::at(
            location("RCP", 1, 2, None, None),
            ErrorCode::DataType,
            "RCP-2 is no number of records above 0 in RD",
        )
    };
    if units.is_some_and(|units| units != "RD") {
        return Err(refused());
    }
    match amount.parse::<u32>() {
        Ok(amount) if amount > 0 => Ok(Some(amount)),
        _ => Err(refused()),
    }
}

/// The ITI-78 parameter each QPD-3 field is searched as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Parameter {
    /// A string or token parameter taking the value as written.
    Plain(&'static str),
    /// `birthdate`, from a v2 date.
    Birthdate,
    /// `gender`, from a table 0001 code.
    Gender,
    /// An identifier's value, `@PID.3.1` or `@PID.18.1`.
    IdentifierValue(u8),
    /// A part of an identifier's assigning authority: the field and the
    /// subcomponent.
    Authority(u8, usize),
}

/// The parameter `name` is searched as (ITI TF-2 Table 3.21-3, Table
/// 3.21-4), `None` for one the Supplier does not search on.
///
/// `@PID.18`, the account number, is searched as an identifier in its
/// assigning authority's domain, the one place the store holds a value of a
/// domain (our own design).
fn parameter(name: &str) -> Option<Parameter> {
    Some(match name {
        "@PID.3.1" => Parameter::IdentifierValue(3),
        "@PID.18.1" => Parameter::IdentifierValue(18),
        "@PID.3.4" | "@PID.3.4.1" => Parameter::Authority(3, 1),
        "@PID.3.4.2" => Parameter::Authority(3, 2),
        "@PID.3.4.3" => Parameter::Authority(3, 3),
        "@PID.18.4" | "@PID.18.4.1" => Parameter::Authority(18, 1),
        "@PID.18.4.2" => Parameter::Authority(18, 2),
        "@PID.18.4.3" => Parameter::Authority(18, 3),
        "@PID.5.1" | "@PID.5.1.1" => Parameter::Plain("family"),
        "@PID.5.2" | "@PID.5.3" => Parameter::Plain("given"),
        "@PID.6.1" | "@PID.6.1.1" => Parameter::Plain("mothersMaidenName"),
        "@PID.7" | "@PID.7.1" => Parameter::Birthdate,
        "@PID.8" => Parameter::Gender,
        "@PID.11.1" | "@PID.11.1.1" | "@PID.11.2" => Parameter::Plain("address"),
        "@PID.11.3" => Parameter::Plain("address-city"),
        "@PID.11.4" => Parameter::Plain("address-state"),
        "@PID.11.5" => Parameter::Plain("address-postalcode"),
        "@PID.11.6" => Parameter::Plain("address-country"),
        "@PID.13.1" | "@PID.13.4" | "@PID.13.12" => Parameter::Plain("telecom"),
        _ => return None,
    })
}

/// The QPD-3 parameters read: the search pairs, and the identifier values
/// and assigning authorities to join.
#[derive(Default)]
struct Parameters {
    /// The search pairs, each `name=value` percent-encoded.
    pairs: Vec<String>,
    /// The identifier values, by PID field.
    identifier_values: BTreeMap<u8, String>,
    /// The assigning authorities, by PID field.
    authorities: BTreeMap<u8, Hd>,
}

/// Reads the QPD-3 parameters of `qpd`.
fn parameters(qpd: Option<&Segment>) -> Result<Parameters, Settled> {
    let mut read = Parameters::default();
    let parameters = qpd
        .and_then(|qpd| qpd.field(3))
        .map(|field| field.repetitions().to_vec())
        .unwrap_or_default();
    for (index, repetition) in parameters.iter().enumerate() {
        let at = || location("QPD", 1, 3, Some(index.saturating_add(1)), None);
        let Some(name) = repetition
            .component(1)
            .and_then(|component| component.text())
        else {
            continue;
        };
        let values: Vec<&str> = repetition
            .component(2)
            .map(|component| {
                component
                    .subcomponents()
                    .iter()
                    .map(String::as_str)
                    .filter(|value| ferrobridge_hl7v2::parse::valued(value))
                    .collect()
            })
            .unwrap_or_default();
        let Some(parameter) = parameter(name) else {
            return Err(Settled::error(vec![Fault::at(
                at(),
                ErrorCode::Application,
                "QPD-3 names a field this Supplier does not search on",
            )]));
        };
        let Some(first) = values.first() else {
            continue;
        };
        match parameter {
            Parameter::Plain(name) => {
                let joined: Vec<String> = values.iter().map(|value| fhir_escape(value)).collect();
                read.pairs
                    .push(format!("{name}={}", url_encoded(&joined.join(","))));
            }
            Parameter::Birthdate => {
                let dates: Option<Vec<String>> =
                    values.iter().map(|value| pid::birth_date(value)).collect();
                let Some(dates) = dates else {
                    return Err(Settled::error(vec![Fault::at(
                        at(),
                        ErrorCode::DataType,
                        "a @PID.7 value is no date",
                    )]));
                };
                read.pairs
                    .push(format!("birthdate={}", url_encoded(&dates.join(","))));
            }
            Parameter::Gender => {
                let Some(gender) = gender(first) else {
                    return Err(Settled::error(vec![Fault::at(
                        at(),
                        ErrorCode::DataType,
                        "the @PID.8 value is no administrative sex",
                    )]));
                };
                read.pairs.push(format!("gender={gender}"));
            }
            Parameter::IdentifierValue(field) => {
                read.identifier_values.insert(field, (*first).to_owned());
            }
            Parameter::Authority(field, part) => {
                let hd = read.authorities.entry(field).or_default();
                let value = Some((*first).to_owned());
                match part {
                    1 => hd.namespace = value,
                    2 => hd.universal = value,
                    _ => hd.kind = value,
                }
            }
        }
    }
    Ok(read)
}

/// A query read as the ITI-78 search.
struct Criteria {
    /// The search's query string, without `_count`.
    search: SecretString,
    /// Whether QPD-8 names the domains returned.
    filtered: bool,
}

/// Reads QPD-3 and QPD-8 as the ITI-78 search, `Ok(None)` for a search no
/// person can match: an identifier in a domain the face does not know.
async fn criteria(face: &Face, message: &Message) -> Result<Option<Criteria>, Settled> {
    let qpd = segment(message, "QPD");
    let Parameters {
        mut pairs,
        identifier_values,
        authorities,
    } = parameters(qpd)?;
    let mut unmatchable = false;
    for (field, value) in &identifier_values {
        let Some(hd) = authorities.get(field).filter(|hd| !hd.is_empty()) else {
            // NOTE: ITI TF-2 §3.21.4.1.2.3, "specifying @PID.3.1 without @PID.3.4 is
            // not allowed" (Appendix E); the field is refused as missing.
            return Err(Settled::error(vec![Fault::at(
                location("QPD", 1, 3, None, None),
                ErrorCode::RequiredFieldMissing,
                "an identifier parameter names no assigning authority",
            )]));
        };
        match face.authorities.system(hd) {
            Some(system) => pairs.push(format!("identifier={}", query_part(&system, value))),
            None => unmatchable = true,
        }
    }
    if pairs.is_empty() && !unmatchable {
        return Err(Settled::error(vec![Fault::at(
            location("QPD", 1, 3, None, None),
            ErrorCode::RequiredFieldMissing,
            "QPD-3 names no parameter",
        )]));
    }
    let domains = domains(face, qpd).await?;
    if unmatchable {
        return Ok(None);
    }
    if !domains.is_empty() {
        let filter: Vec<String> = domains
            .iter()
            .map(|system| format!("{}|", fhir_escape(system)))
            .collect();
        pairs.push(format!("identifier={}", url_encoded(&filter.join(","))));
    }
    Ok(Some(Criteria {
        search: SecretString::from(pairs.join("&")),
        filtered: !domains.is_empty(),
    }))
}

/// Reads QPD-8, the domains whose identifiers are returned, refusing every
/// one the Supplier does not recognize (ITI TF-2 §3.21.4.2.2.8, Case 3):
/// a domain the store holds and has not retired, or the Master Patient
/// Identification Domain, as ITI-78 recognizes them.
async fn domains(face: &Face, qpd: Option<&Segment>) -> Result<Vec<String>, Settled> {
    let repetitions = qpd
        .and_then(|qpd| qpd.field(8))
        .map(|field| field.repetitions().to_vec())
        .unwrap_or_default();
    if repetitions.iter().all(|repetition| !repetition.is_valued()) {
        return Ok(Vec::new());
    }
    let known = record::domains(face.store.pool()).await.map_err(|error| {
        tracing::error!(error = %crate::chain(&error), "the domains could not be read for an ITI-21 query");
        Settled::rejected("the store could not be read")
    })?;
    let mut systems = Vec::new();
    let mut faults = Vec::new();
    for (index, repetition) in repetitions.iter().enumerate() {
        if !repetition.is_valued() {
            continue;
        }
        let system = face
            .authorities
            .system(&Hd::of(repetition.component(4)))
            .filter(|system| {
                *system == face.master || known.iter().any(|d| d.system == *system && !d.retired)
            });
        match system {
            Some(system) => systems.push(system),
            None => faults.push(Fault::at(
                location("QPD", 1, 8, Some(index.saturating_add(1)), None),
                ErrorCode::UnknownKeyIdentifier,
                "a domain of QPD-8 is not recognized",
            )),
        }
    }
    if faults.is_empty() {
        Ok(systems)
    } else {
        Err(Settled::error(faults))
    }
}

/// Runs `search` after `after`, an increment of `increment` persons or all
/// of them, and returns them with the last person when more follow.
async fn run(
    face: &Face,
    search: &SecretString,
    mut after: Option<Uuid>,
    increment: Option<u32>,
) -> Result<(Vec<Found>, Option<Uuid>), Settled> {
    let mut found: Vec<Found> = Vec::new();
    loop {
        let wanted = increment.map_or(MAX_PAGE, |increment| {
            increment
                .saturating_sub(u32::try_from(found.len()).unwrap_or(u32::MAX))
                .min(MAX_PAGE)
        });
        let text = if search.expose_secret().is_empty() {
            format!("_count={wanted}")
        } else {
            format!("{}&_count={wanted}", search.expose_secret())
        };
        let parsed = PatientSearch::from_query(&text).map_err(|error| {
            tracing::debug!(%error, "an ITI-21 query reads as no ITI-78 search");
            Settled::error(vec![Fault::at(
                location("QPD", 1, 3, None, None),
                ErrorCode::DataType,
                "a QPD-3 value cannot be searched on",
            )])
        })?;
        let searched = face
            .supplier
            .search(&parsed, after)
            .await
            .map_err(|error| {
                tracing::error!(error = %crate::chain(&error), "an ITI-21 search failed");
                Settled::rejected("the store could not be read")
            })?;
        let Searched::Found { answer, persons } = searched else {
            return Err(Settled::error(vec![Fault::at(
                location("QPD", 1, 8, None, None),
                ErrorCode::UnknownKeyIdentifier,
                "a domain of QPD-8 is not recognized",
            )]));
        };
        let bundle = answer.resource();
        let more = bundle
            .link
            .iter()
            .any(|link| link.relation.value.as_deref() == Some("next"));
        let mut patients: BTreeMap<String, Object> = BTreeMap::new();
        for entry in &bundle.entry {
            if let Some(Resource::Patient(patient)) = &entry.resource
                && let Some(id) = patient.id.clone()
                && let Ok(object) = patient.to_json()
            {
                patients.insert(id, object);
            }
        }
        for person in &persons {
            if let Some(patient) = patients.remove(&person.to_string()) {
                found.push((*person, patient));
            }
        }
        after = persons.last().copied().or(after);
        let full = increment.is_some_and(|increment| {
            found.len() >= usize::try_from(increment).unwrap_or(usize::MAX)
        });
        if !more {
            return Ok((found, None));
        }
        if full {
            return Ok((found, after));
        }
    }
}

/// The FHIR `administrative-gender` of a table 0001 code (v2-to-FHIR
/// `table-hl70001-to-administrative-gender`).
fn gender(code: &str) -> Option<&'static str> {
    Some(match code {
        "F" => "female",
        "M" => "male",
        "O" | "A" | "N" => "other",
        "U" => "unknown",
        _ => return None,
    })
}

/// The identifiers of a `Patient`'s JSON, each with its system.
fn identifiers(patient: &Object) -> Vec<(String, String)> {
    patient
        .get("identifier")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|identifier| {
            Some((
                identifier.get("system")?.as_str()?.to_owned(),
                identifier.get("value")?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

/// The first segment `id` of `message`.
fn segment<'m>(message: &'m Message, id: &str) -> Option<&'m Segment> {
    message.segments().iter().find(|segment| segment.id() == id)
}

/// Writes the RSP^K22 of `settled`.
fn answer(message: &Message, settled: &Settled, face: &Face, stamped: &Stamped) -> Answer {
    let writer = Writer::of(message);
    let mut segments = vec![
        writer.header(message, &["RSP", "K22", "RSP_K21"], stamped),
        writer.msa(message, settled.code.as_str()),
    ];
    segments.extend(settled.faults.iter().map(|fault| writer.err(fault)));
    segments.push(writer.segment(
        "QAK",
        &[writer.echoed(message, "QPD", 2), settled.status.to_owned()],
    ));
    if let Some(qpd) = writer.echo_segment(message, "QPD") {
        segments.push(qpd);
    }
    for (index, (_, patient, identifiers)) in settled.found.iter().enumerate() {
        segments.push(pid::segment(
            &writer,
            &face.authorities,
            index.saturating_add(1),
            identifiers,
            Some(patient),
            Shown::Demographics,
        ));
    }
    if let Some(pointer) = &settled.pointer {
        segments.push(writer.segment("DSC", &[writer.escape(pointer), String::from("I")]));
    }
    Answer {
        code: settled.code,
        text: Writer::message(&segments),
    }
}

#[cfg(test)]
mod tests {
    use super::{Parameter, parameter};

    #[test]
    fn every_field_of_table_3_21_3_is_searched_on() {
        for name in [
            "@PID.3.1",
            "@PID.5.1.1",
            "@PID.7",
            "@PID.8",
            "@PID.11.5",
            "@PID.18.1",
        ] {
            assert!(parameter(name).is_some(), "{name}");
        }
        assert_eq!(
            parameter("@PID.6.1"),
            Some(Parameter::Plain("mothersMaidenName"))
        );
        assert_eq!(parameter("@PID.13.1"), Some(Parameter::Plain("telecom")));
        assert_eq!(parameter("@PID.10"), None);
        assert_eq!(parameter("@PD1.4"), None);
    }
}
