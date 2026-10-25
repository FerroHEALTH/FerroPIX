// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ITI-8, the Patient Identity Feed, as the Manager receives it.
//!
//! An add or revise (ADT^A01, A04, A05, A08; ITI TF-2 §3.8.4.1.1) is read
//! as the ITI-104 Add or Revise Patient message of the same patient, and a
//! merge (ADT^A40, §3.8.4.2.1) as its Resolve Duplicate Patient message:
//! MRG-1 subsumed into the PID-3 identifier, whose references replace the
//! subsumed one's before the cross-referencing runs again (§3.8.4.2.3). The
//! identifier fed is the PID-3 repetition in the domain of the sender; a
//! repetition with no assigning authority takes it (§3.8.4.1.3), and a
//! message that carries none in that domain is refused. The message is then
//! applied by the feed transaction ITI-104 goes through, so both faces
//! write the same version and join the same person. Each message is a
//! Manager audit record (§3.8.5.1.2, §3.8.5.2.2) and a receipt, so a
//! message sent again is acknowledged again and not applied twice.

use ferrobridge_hl7v2::ack::Code;
use ferrobridge_hl7v2::parse::{Message, Segment};
use fhir_types::codec::Value;
use ihe_atna::balp::{Exchange, Outcome};
use ihe_iti::pixm::server::feed::Feed;
use ihe_iti::server::format::Format;
use secrecy::SecretString;

use crate::audit::records::{self, Identifier};
use crate::config::section::hl7v2::Sender;
use crate::feed::{Action, Applied, Context, FeedError};
use crate::hl7v2::pid::{self, Cx};
use crate::hl7v2::stamped::Stamped;
use crate::hl7v2::write::{Condition, Fault, location};
use crate::hl7v2::{Answer, Face, acknowledge, receipt, sender_key};
use crate::store::record::Channel;

/// What applying a message settled: the code, the faults, and whether the
/// outcome is final, so its receipt is written.
struct Settled {
    /// MSA-1.
    code: Code,
    /// The ERR segments.
    faults: Vec<Fault>,
    /// Whether a resend is answered with this code without being applied.
    final_outcome: bool,
}

impl Settled {
    /// A refusal that is final: the same message is refused again.
    fn refused(fault: Fault) -> Self {
        Self {
            code: Code::Error,
            faults: vec![fault],
            final_outcome: true,
        }
    }

    /// A refusal for a reason that may pass: no receipt, so a resend is
    /// applied.
    fn rejected(text: &'static str) -> Self {
        Self {
            code: Code::Reject,
            faults: vec![Fault::anywhere(Condition::Application, text)],
            final_outcome: false,
        }
    }
}

/// Answers one ITI-8 message of `sender`; `merge` for an A40.
pub(super) async fn receive(
    face: &Face,
    message: &Message,
    sender: &Sender,
    merge: bool,
    stamped: &Stamped,
) -> Answer {
    let key = sender_key(sender);
    let Some(control_id) = message.control_id() else {
        return acknowledge(
            message,
            Code::Error,
            &[Fault::at(
                location("MSH", 1, 10, None, None),
                Condition::RequiredFieldMissing,
                "MSH-10 is required to acknowledge the message",
            )],
            stamped,
        );
    };
    match receipt::answered(face.store.pool(), &key, control_id).await {
        Ok(Some(code)) => {
            tracing::info!(
                ack = code.as_str(),
                "a message sent again is not applied again"
            );
            let (code, faults) = match code.as_str() {
                "AA" => (Code::Accept, Vec::new()),
                answered => (
                    if answered == "AR" {
                        Code::Reject
                    } else {
                        Code::Error
                    },
                    vec![Fault::anywhere(
                        Condition::Application,
                        "the message was answered before and is not applied again",
                    )],
                ),
            };
            return acknowledge(message, code, &faults, stamped);
        }
        Ok(None) => {}
        Err(error) => {
            tracing::error!(error = %crate::chain(&error), "the receipts could not be read");
            let settled = Settled::rejected("the store could not be read; send the message again");
            return acknowledge(message, settled.code, &settled.faults, stamped);
        }
    }
    let settled = apply(face, message, sender, merge).await;
    if settled.final_outcome
        && let Err(error) =
            receipt::record(face.store.pool(), &key, control_id, settled.code.as_str()).await
    {
        tracing::warn!(error = %crate::chain(&error), "the receipt of a message could not be written");
    }
    acknowledge(message, settled.code, &settled.faults, stamped)
}

/// Reads, applies and audits one ITI-8 message.
async fn apply(face: &Face, message: &Message, sender: &Sender, merge: bool) -> Settled {
    let Some(domain) = sender.feeds() else {
        return Settled::refused(Fault::anywhere(
            Condition::Application,
            "the sender feeds no identifier domain",
        ));
    };
    let read = match read(face, message, domain, merge) {
        Ok(read) => read,
        Err(fault) => return Settled::refused(fault),
    };
    let place = match face.audit.reserve() {
        Ok(place) => place,
        Err(error) => {
            tracing::error!(error = %crate::chain(&error), "an ITI-8 message is refused: its audit record has no room");
            return Settled::rejected(
                "the audit record could not be stored, so the message was not applied",
            );
        }
    };
    let context = Context {
        actor: sender_key(sender),
        channel: Channel::V2,
        request_id: message.control_id().map(str::to_owned),
    };
    let applied = match &read.feed {
        Feed::Revise(revision) => face.feeder.revise(revision, &context).await,
        Feed::ResolveDuplicate(resolved) => face.feeder.resolve_duplicate(resolved, &context).await,
        _ => Err(FeedError::Evidence),
    };
    let call = face.call(sender, message);
    let exchanges = records(&call, &read, applied.as_ref());
    let mut exchanges = exchanges.into_iter();
    let recorded = match exchanges.next() {
        Some(first) => match place.record(&first).await {
            Ok(()) => face.audit.record(exchanges.collect()).await,
            Err(error) => Err(error),
        },
        None => Ok(()),
    };
    if let Err(error) = recorded {
        tracing::error!(error = %crate::chain(&error), "the audit record of an ITI-8 message could not be stored");
        return if applied.is_ok() {
            Settled::refused(Fault::anywhere(
                Condition::Application,
                "the message was applied, and its audit record could not be stored",
            ))
        } else {
            Settled::rejected(
                "the audit record could not be stored, so the message was not applied",
            )
        };
    }
    match applied {
        Ok(applied) => {
            tracing::info!(action = ?applied.action, "an ITI-8 message is applied");
            Settled {
                code: Code::Accept,
                faults: Vec::new(),
                final_outcome: true,
            }
        }
        Err(error) => refusal(&error),
    }
}

/// One ITI-8 message read as its ITI-104 message.
struct Read {
    /// The ITI-104 message.
    feed: Feed,
    /// The identifier fed: PID-3's in the sender's domain, the survivor of
    /// a merge.
    own: Identifier,
    /// MRG-1, the subsumed identifier of a merge.
    subsumed: Option<Identifier>,
}

/// Reads `message` as the ITI-104 message of the same patient, fed into
/// `domain`.
fn read(face: &Face, message: &Message, domain: &str, merge: bool) -> Result<Read, Fault> {
    let pid = segment(message, "PID").ok_or_else(|| {
        Fault::at(
            location("PID", 1, 3, None, None),
            Condition::RequiredFieldMissing,
            "the message carries no PID segment",
        )
    })?;
    let carried = pid::identifiers(pid.field(3), &face.authorities, domain);
    if carried.is_empty() {
        return Err(Fault::at(
            location("PID", 1, 3, None, None),
            Condition::RequiredFieldMissing,
            "PID-3 carries no identifier",
        ));
    }
    let own = carried
        .iter()
        .find(|cx| cx.system.as_deref() == Some(domain))
        .cloned()
        .ok_or_else(|| {
            Fault::at(
                location("PID", 1, 3, Some(1), Some(4)),
                Condition::UnknownKey,
                "PID-3 carries no identifier in the domain the sender feeds",
            )
        })?;
    let known: Vec<(String, String)> = carried
        .iter()
        .filter_map(|cx| Some((cx.system.clone()?, cx.value.clone())))
        .collect();
    let (identifiers, links, subsumed) = if merge {
        let mrg = segment(message, "MRG").ok_or_else(|| {
            Fault::at(
                location("MRG", 1, 1, None, None),
                Condition::RequiredFieldMissing,
                "an A40 carries the subsumed identifier in MRG-1",
            )
        })?;
        let subsumed = pid::identifiers(mrg.field(1), &face.authorities, domain)
            .into_iter()
            .next()
            .ok_or_else(|| {
                Fault::at(
                    location("MRG", 1, 1, None, None),
                    Condition::RequiredFieldMissing,
                    "MRG-1 carries no identifier",
                )
            })?;
        if subsumed.system.as_deref() != Some(domain) {
            return Err(Fault::at(
                location("MRG", 1, 1, Some(1), Some(4)),
                Condition::UnknownKey,
                "MRG-1 is not in the domain the sender feeds",
            ));
        }
        let mut identifiers = vec![(domain.to_owned(), subsumed.value.clone())];
        identifiers.extend(known.into_iter().filter(|(system, _)| system != domain));
        let links = vec![pid::replaced_by(domain, &own.value)];
        (identifiers, links, Some(subsumed))
    } else {
        (known, Vec::new(), None)
    };
    let patient = pid::patient(pid, &identifiers, links).map_err(|_date| {
        Fault::at(
            location("PID", 1, 7, None, None),
            Condition::DataType,
            "PID-7 is no date",
        )
    })?;
    let condition = subsumed.as_ref().unwrap_or(&own);
    let body = serde_json::to_vec(&Value::Object(patient)).map_err(|_json| {
        Fault::anywhere(Condition::Application, "the patient could not be written")
    })?;
    let query = format!("identifier={}", query_part(domain, &condition.value));
    let feed = Feed::from_update(&query, Format::Json, &body).map_err(|error| {
        tracing::debug!(%error, "an ITI-8 message is no feed the Manager applies");
        Fault::at(
            location("PID", 1, 3, None, None),
            Condition::Application,
            error.to_string(),
        )
    })?;
    Ok(Read {
        feed,
        own: identifier(domain, &own),
        subsumed: subsumed.as_ref().map(|cx| identifier(domain, cx)),
    })
}

/// Returns the records of a message read as `read` that ended in `applied`
/// (ITI TF-2 §3.8.5.1.2: `C` for a new identifier, `U` for a known one;
/// §3.8.5.2.2: a merge's `U` for the survivor and `D` for the subsumed
/// identifier).
fn records(
    call: &records::Call,
    read: &Read,
    applied: Result<&Applied, &FeedError>,
) -> Vec<Exchange> {
    match (applied, &read.subsumed) {
        (Ok(applied), None) => {
            let action = match applied.action {
                Action::Create => records::Feed::Create,
                _ => records::Feed::Update,
            };
            vec![records::v2_feed(call, action, &read.own, Outcome::Success)]
        }
        (Ok(_), Some(subsumed)) => vec![
            records::v2_feed(call, records::Feed::Update, &read.own, Outcome::Success),
            records::v2_feed(call, records::Feed::Delete, subsumed, Outcome::Success),
        ],
        (Err(_), subsumed) => vec![records::v2_feed(
            call,
            records::Feed::Update,
            subsumed.as_ref().unwrap_or(&read.own),
            Outcome::MinorFailure,
        )],
    }
}

/// The acknowledgment of a message the feed transaction did not apply.
fn refusal(error: &FeedError) -> Settled {
    let identifier = || location("PID", 1, 3, Some(1), Some(1));
    match error {
        FeedError::UnknownDomain => Settled::refused(Fault::at(
            location("PID", 1, 3, Some(1), Some(4)),
            Condition::UnknownKey,
            "the domain the sender feeds is not known to the store",
        )),
        FeedError::Identifier { .. } => Settled::refused(Fault::at(
            identifier(),
            Condition::DataType,
            "an identifier is refused by its domain's rule",
        )),
        FeedError::Subsumed => Settled::refused(Fault::at(
            identifier(),
            Condition::Application,
            "the identifier was merged into another and takes no new version",
        )),
        FeedError::SurvivorNotFound | FeedError::NotFound => Settled::refused(Fault::at(
            identifier(),
            Condition::UnknownKey,
            "the surviving identifier names no active record of the domain",
        )),
        FeedError::Demographics(_) | FeedError::Evidence => Settled::refused(Fault::anywhere(
            Condition::Application,
            "the demographics could not be stored",
        )),
        FeedError::Contention => {
            Settled::rejected("concurrent messages moved the same persons; send the message again")
        }
        _ => {
            tracing::error!(error = %crate::chain(error), "an ITI-8 message could not be applied");
            Settled::rejected("the store could not apply the message; send it again")
        }
    }
}

/// The first segment `id` of `message`.
fn segment<'m>(message: &'m Message, id: &str) -> Option<&'m Segment> {
    message.segments().iter().find(|segment| segment.id() == id)
}

/// The audit identifier of `cx` in `domain`.
fn identifier(domain: &str, cx: &Cx) -> Identifier {
    Identifier {
        system: domain.to_owned(),
        value: SecretString::from(cx.value.clone()),
    }
}

/// The token `system|value`, its separators escaped (FHIR R4 search,
/// "Escaping Search Parameters") and percent-encoded for a URL query.
#[must_use]
pub(super) fn query_part(system: &str, value: &str) -> String {
    let token = format!("{}|{}", fhir_escape(system), fhir_escape(value));
    url_encoded(&token)
}

/// `text` with the characters a FHIR search value escapes, `\`, `|`, `,`
/// and `$`, escaped with a backslash.
#[must_use]
pub(super) fn fhir_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '\\' | '|' | ',' | '$') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// `text` percent-encoded for a URL query, a space as `%20`.
#[must_use]
pub(super) fn url_encoded(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

#[cfg(test)]
mod tests {
    use super::{fhir_escape, query_part};

    #[test]
    fn a_condition_token_escapes_its_separators() {
        assert_eq!(fhir_escape("a|b,c$d\\e"), "a\\|b\\,c\\$d\\\\e");
        assert_eq!(
            query_part("urn:oid:2.999.1.1", "A 1"),
            "urn%3Aoid%3A2.999.1.1%7CA%201"
        );
    }
}
