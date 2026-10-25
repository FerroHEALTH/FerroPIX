// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The HL7 v2 face: one MLLP listener with ITI-8, ITI-9 and ITI-21 on the
//! store the FHIR face serves (`docs/architecture.md` §7).
//!
//! Each frame runs through FerroBRIDGE's `ferrobridge_hl7v2`: the MLLP
//! Release 1 codec and listener, the MSH-18 decoder and the positional
//! parser over the `hl7v2-types` tables, which answer a message they cannot
//! read with `AR` or `AE` themselves. A message they parse is then taken by
//! its sender, MSH-3 and MSH-4, which `[[hl7v2.senders]]` must name (a
//! message from another is acknowledged `AR` and not applied), and by its
//! type:
//!
//! - ADT^A01, A04, A05 and A08 ([`feed`]): ITI-8's add or revise (ITI TF-2
//!   §3.8.4.1), and ADT^A40, ITI-8's merge (§3.8.4.2), applied by the feed
//!   transaction an ITI-104 message goes through, acknowledged in original
//!   mode.
//! - QBP^Q23 ([`pix`]): ITI-9, answered with RSP^K23 from the
//!   cross-reference ITI-83 reads (§3.9.4).
//! - QBP^Q22 and QCN^J01 ([`pdq`]): ITI-21, answered with RSP^K22 from the
//!   search ITI-78 runs, with the continuation protocol (§3.21.4).
//!
//! Every other type is acknowledged `AR` with `ERR` code `200`. No byte of a
//! message reaches a log line: the span names MSH-10 and the type, and the
//! events the acknowledgment code.

pub mod authority;
pub mod cursor;
pub mod feed;
pub mod pdq;
pub mod pid;
pub mod pix;
pub mod receipt;
pub mod stamped;
pub mod write;

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ferrobridge_hl7v2::ack::{self, Code, Header};
use ferrobridge_hl7v2::decode::{self, Charset};
use ferrobridge_hl7v2::inbound::{self, Inbound, Received};
use ferrobridge_hl7v2::mllp::{self, Codec, Connection, Handler, Malformed, Timeouts};
use ferrobridge_hl7v2::parse::structure::{StructureError, structure_for};
use ferrobridge_hl7v2::parse::{Component, Message};
use ferropix_operator_api::health::IndicatorState;
use ihe_atna::balp::{NetworkAddress, Peer};
use ihe_atna::user::OnBehalfOf;
use tokio::net::TcpListener;
use tracing::Instrument as _;
use url::Url;

use crate::audit::Audit;
use crate::audit::records::Call;
use crate::config::section::hl7v2::{Hl7v2Settings, Sender};
use crate::config::section::matching::Matching;
use crate::config::section::profiles::Deprecated;
use crate::feed::Feeder;
use crate::health::{Check, HealthIndicator};
use crate::hl7v2::authority::Authorities;
use crate::hl7v2::cursor::Cursors;
use crate::hl7v2::stamped::Stamped;
use crate::hl7v2::write::{Condition, Fault, Writer};
use crate::pdqm::Supplier;
use crate::store::Store;
use crate::usage_report::window::Window;

/// The answer to one message: its acknowledgment code and its text, before
/// it is encoded in the message's character set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// MSA-1.
    pub code: Code,
    /// The answer's segments.
    pub text: String,
}

/// The face: what one frame's answer is built from.
#[derive(Debug)]
pub struct Face {
    /// The cross-reference store.
    store: Store,
    /// The feed transaction ITI-104 goes through.
    feeder: Feeder,
    /// The PDQm Supplier, whose search ITI-21 runs.
    supplier: Supplier,
    /// The audit trail.
    audit: Audit,
    /// The character set a message with an empty MSH-18 is read in.
    charset: Charset,
    /// The senders accepted.
    senders: Vec<Sender>,
    /// The namespace ids of the domains.
    authorities: Authorities,
    /// The continuation pointers of ITI-21.
    cursors: Cursors,
    /// The listener's own address, as the records name it.
    endpoint: NetworkAddress,
    /// The system of the Master Patient Identification Domain.
    master: String,
    /// The usage report's window each message is counted in.
    usage: Option<Arc<Window>>,
}

impl Face {
    /// Returns the face over `store` that `settings` describes, scoring with
    /// `matching`, answering a merged person by `deprecated`, recording to
    /// `audit`, at `public_base_url`.
    #[must_use]
    pub fn new(
        store: &Store,
        matching: &Matching,
        deprecated: Deprecated,
        public_base_url: &Url,
        audit: &Audit,
        settings: &Hl7v2Settings,
    ) -> Self {
        Self {
            store: store.clone(),
            feeder: Feeder::new(store.clone(), matching),
            supplier: Supplier::new(store.clone(), matching, deprecated, public_base_url),
            audit: audit.clone(),
            charset: settings.default_charset,
            senders: settings.senders.clone(),
            authorities: Authorities::new(&settings.namespaces),
            cursors: Cursors::default(),
            endpoint: NetworkAddress::Ip(settings.listen.ip()),
            master: public_base_url.as_str().trim_end_matches('/').to_owned(),
            usage: None,
        }
    }

    /// Returns the face keeping its continuation pointers in `cursors`.
    #[must_use]
    pub fn with_cursors(mut self, cursors: Cursors) -> Self {
        self.cursors = cursors;
        self
    }

    /// Returns the face counting its messages in `usage`, the window the
    /// server's usage report takes.
    #[must_use]
    pub fn with_usage(mut self, usage: Arc<Window>) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Answers one frame's bytes: the answer's bytes, or `None` when no
    /// header can be read to address one.
    pub async fn answer(&self, message: &[u8], connection: &Connection) -> Option<Vec<u8>> {
        let header = Header::from_bytes(message);
        let span = tracing::info_span!(
            "hl7v2_message",
            control_id = header.as_ref().and_then(Header::control_id),
            message_type = header.as_ref().and_then(Header::type_code),
        );
        let started = Instant::now();
        let answered = async {
            let stamped = Stamped::now();
            match inbound::receive(message, self.charset, select, stamped.stamp()) {
                Received::Unanswerable => {
                    tracing::warn!("the message names no readable header and is not answered");
                    None
                }
                Received::Answered { code, reply } => {
                    tracing::info!(ack = code.as_str(), "answered before it was read");
                    Some((code, reply))
                }
                Received::Parsed(inbound) => {
                    let answer = self.settle(&inbound, message, connection, &stamped).await;
                    tracing::info!(ack = answer.code.as_str(), "message answered");
                    Some(encoded(inbound.parsed().message(), &answer, &stamped))
                }
            }
        }
        .instrument(span)
        .await;
        if let Some(usage) = &self.usage {
            usage.record_message(started.elapsed(), answered.as_ref().map(|(code, _)| *code));
        }
        answered.map(|(_, reply)| reply)
    }

    /// Answers one parsed message by its sender and type.
    async fn settle(
        &self,
        inbound: &Inbound,
        raw: &[u8],
        connection: &Connection,
        stamped: &Stamped,
    ) -> Answer {
        let message = inbound.parsed().message();
        let Some(sender) = self.senders.iter().find(|sender| names(sender, message)) else {
            connection.refuse();
            return acknowledge(
                message,
                Code::Reject,
                &[Fault::anywhere(
                    Condition::Application,
                    "the sending application and facility are not accepted",
                )],
                stamped,
            );
        };
        match (message.message_type(1), message.message_type(2)) {
            (Some("ADT"), Some("A01" | "A04" | "A05" | "A08")) => {
                feed::receive(self, message, sender, false, stamped).await
            }
            (Some("ADT"), Some("A40")) => feed::receive(self, message, sender, true, stamped).await,
            (Some("QBP"), Some("Q23")) => {
                pix::query(self, message, sender, &self.text(raw), stamped).await
            }
            (Some("QBP"), Some("Q22")) => {
                pdq::query(self, message, sender, &self.text(raw), stamped).await
            }
            (Some("QCN"), Some("J01")) => pdq::cancel(self, message, sender, stamped),
            _ => acknowledge(
                message,
                Code::Reject,
                &[Fault::anywhere(
                    Condition::UnsupportedMessageType,
                    "the message type and trigger event are not ones this Manager answers",
                )],
                stamped,
            ),
        }
    }

    /// Returns the message text the records carry, decoded as the parser
    /// read it.
    fn text(&self, raw: &[u8]) -> String {
        decode::decode(raw, self.charset).map_or_else(
            |_| String::from_utf8_lossy(raw).into_owned(),
            |decoded| decoded.text,
        )
    }

    /// Returns the call the records of `sender`'s message name: the sender
    /// as the facility and application joined by `|` (ITI TF-2 §3.8.5.1.2,
    /// Source `UserID`), at the listener's address, since the listener does
    /// not hand the peer's, and MSH-10 as the request id.
    fn call(&self, sender: &Sender, message: &Message) -> Call {
        Call {
            client: Peer {
                who: sender_key(sender),
                network: self.endpoint.clone(),
            },
            on_behalf: OnBehalfOf::System,
            endpoint: self.endpoint.clone(),
            request_id: message.control_id().map(str::to_owned),
        }
    }
}

impl Handler for Face {
    async fn handle(&self, message: Vec<u8>, connection: &Connection) -> Option<Vec<u8>> {
        self.answer(&message, connection).await
    }

    fn malformed(&self, kind: Malformed, partial: &[u8]) -> Option<Vec<u8>> {
        let stamped = Stamped::now();
        let reply = ack::reject_frame(partial, &kind.to_string(), stamped.stamp());
        if let Some(usage) = &self.usage
            && reply.is_some()
        {
            usage.record_message(Duration::ZERO, Some(Code::Reject));
        }
        reply
    }
}

/// Selects the message structure of `message`, ITI-8's `ADT_A01` included.
///
/// The definitions give A04, A05 and A08 structures of their own, and ITI-8
/// names `ADT_A01` for every add or revise (ITI TF-2 §3.8.4.1.2.1: the third
/// component "shall have the value of `ADT_A01` for all message types"), so
/// that name selects the A01 structure for them.
///
/// # Errors
/// Returns the [`StructureError`] of a structure the definitions do not
/// carry.
pub fn select(message: &Message) -> Result<&'static hl7v2_types::model::Structure, StructureError> {
    structure_for(message).or_else(|error| {
        let feed = matches!(
            (
                message.message_type(1),
                message.message_type(2),
                message.message_type(3)
            ),
            (
                Some("ADT"),
                Some("A01" | "A04" | "A05" | "A08"),
                Some("ADT_A01")
            )
        );
        if !feed {
            return Err(error);
        }
        hl7v2_types::message::find("ADT", "A01")
            .and_then(|definition| definition.structure)
            .ok_or(error)
    })
}

/// Encodes `answer` in the character set of `message`; an answer that holds
/// a character the set lacks is replaced by an `AE` that says so.
fn encoded(message: &Message, answer: &Answer, stamped: &Stamped) -> (Code, Vec<u8>) {
    let charset = message.charset();
    if let Ok(bytes) = decode::encode(&answer.text, charset) {
        return (answer.code, bytes);
    }
    tracing::warn!("the answer holds a character the message's character set lacks");
    let fallback = acknowledge(
        message,
        Code::Error,
        &[Fault::anywhere(
            Condition::Application,
            "the answer holds a character the message's character set cannot carry",
        )],
        stamped,
    );
    let bytes =
        decode::encode(&fallback.text, charset).unwrap_or_else(|_| fallback.text.into_bytes());
    (fallback.code, bytes)
}

/// Returns the name a sender's records and receipts carry: its facility
/// and application joined by `|` (ITI TF-2 §3.8.5.1.2).
#[must_use]
pub fn sender_key(sender: &Sender) -> String {
    format!("{}|{}", sender.facility, sender.application)
}

/// Returns whether `sender` names the sending application and facility of
/// `message`: MSH-3 and MSH-4, each by its namespace id or universal id.
#[must_use]
pub fn names(sender: &Sender, message: &Message) -> bool {
    let named = |position: usize, wanted: &str| {
        message
            .header()
            .and_then(|header| header.field(position))
            .and_then(|field| field.repetitions().first())
            .is_some_and(|hd| {
                [1, 2]
                    .into_iter()
                    .any(|part| hd.component(part).and_then(Component::text) == Some(wanted))
            })
    };
    named(3, &sender.application) && named(4, &sender.facility)
}

/// Returns the acknowledgment of `message`: `ACK` with the message's
/// trigger event, MSA with `code`, and one `ERR` per fault.
#[must_use]
pub fn acknowledge(message: &Message, code: Code, faults: &[Fault], stamped: &Stamped) -> Answer {
    let writer = Writer::of(message);
    let event = message.message_type(2).unwrap_or_default();
    let mut segments = vec![
        writer.header(message, &["ACK", event, "ACK"], stamped),
        writer.msa(message, code.as_str()),
    ];
    segments.extend(faults.iter().map(|fault| writer.err(fault)));
    Answer {
        code,
        text: Writer::message(&segments),
    }
}

/// Whether the MLLP listener's socket is accepting, the readiness indicator
/// of the face (`docs/architecture.md` §9).
#[derive(Debug, Clone, Default)]
pub struct Listening {
    /// Up while the listener accepts.
    up: Arc<AtomicBool>,
}

impl Listening {
    /// Returns whether the listener is accepting.
    #[must_use]
    pub fn is_up(&self) -> bool {
        self.up.load(Ordering::Acquire)
    }

    /// Records whether the listener is accepting.
    fn set(&self, up: bool) {
        self.up.store(up, Ordering::Release);
    }
}

impl HealthIndicator for Listening {
    fn name(&self) -> &'static str {
        "hl7v2_listener"
    }

    fn check(&self) -> Check<'_> {
        Box::pin(async move {
            if self.is_up() {
                IndicatorState::up()
            } else {
                IndicatorState::down("the MLLP listener is not accepting")
            }
        })
    }
}

/// How the MLLP listener runs.
#[derive(Debug, Clone, Copy)]
pub struct Listener {
    /// The frame codec, with its ceiling.
    pub codec: Codec,
    /// How long a connection and a frame may wait on the peer.
    pub timeouts: Timeouts,
    /// How long the drain may take after the stop signal.
    pub drain: Duration,
}

impl Listener {
    /// Returns the listener `settings` describe, draining within `drain`,
    /// the HTTP server's drain window.
    #[must_use]
    pub const fn of(settings: &Hl7v2Settings, drain: Duration) -> Self {
        Self {
            codec: Codec::new(settings.frame_limit),
            timeouts: Timeouts {
                idle: Some(settings.idle_timeout),
                frame: Some(settings.frame_timeout),
            },
            drain,
        }
    }
}

/// Serves `face` on an already-bound `socket` until `shutdown` completes,
/// then lets every connection finish the message it is answering within the
/// drain.
///
/// A message in flight when the signal arrives is applied and acknowledged;
/// a connection still open when the drain elapses is dropped, as the HTTP
/// server's are. `listening` reads up from the first accept until the
/// signal. No specification governs the drain: our own design.
///
/// # Errors
///
/// Returns the I/O error of a failed `accept`.
pub async fn serve_until<F>(
    socket: TcpListener,
    face: Arc<Face>,
    listener: Listener,
    listening: Listening,
    shutdown: F,
) -> std::io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let signalled = Arc::new(tokio::sync::Notify::new());
    let inner = Arc::clone(&signalled);
    let flag = listening.clone();
    listening.set(true);
    let server = mllp::serve_with(
        socket,
        face,
        listener.codec,
        listener.timeouts,
        async move {
            shutdown.await;
            flag.set(false);
            inner.notify_one();
        },
    );
    let mut server = std::pin::pin!(server);
    let result = tokio::select! {
        result = &mut server => Some(result),
        () = signalled.notified() => None,
    };
    if let Some(result) = result {
        listening.set(false);
        return result;
    }
    if let Ok(result) = tokio::time::timeout(listener.drain, server).await {
        return result;
    }
    tracing::warn!(
        drain_ms = listener.drain.as_millis(),
        "the MLLP drain did not finish in time; the remaining connections are dropped"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use ferrobridge_hl7v2::ack::Code;
    use ferrobridge_hl7v2::decode::Charset;
    use ferrobridge_hl7v2::parse::lex::lex;

    use super::{acknowledge, names, sender_key};
    use crate::config::section::hl7v2::Sender;
    use crate::hl7v2::stamped::Stamped;
    use crate::hl7v2::write::{Condition, Fault};

    fn sender(application: &str, facility: &str) -> Sender {
        Sender {
            application: application.to_owned(),
            facility: facility.to_owned(),
            domain: String::from("urn:oid:2.999.1.1"),
        }
    }

    #[test]
    fn a_sender_is_named_by_the_namespace_or_universal_id_of_msh_3_and_msh_4() {
        let message = lex(
            "MSH|^~\\&|ADT^1.2.3^ISO|EAST|PIX|NORTH|20261025||ADT^A08^ADT_A01|M-2|P|2.5\r",
            Charset::Ascii,
        )
        .expect("the header lexes")
        .message;
        assert!(names(&sender("ADT", "EAST"), &message));
        assert!(names(&sender("1.2.3", "EAST"), &message));
        assert!(!names(&sender("ADT", "WEST"), &message));
        assert!(!names(&sender("LAB", "EAST"), &message));
        assert_eq!(sender_key(&sender("ADT", "EAST")), "EAST|ADT");
    }

    #[test]
    fn an_acknowledgment_names_the_trigger_event_and_echoes_msh_10() {
        let message = lex(
            "MSH|^~\\&|ADT|EAST|PIX|NORTH|20261025||ADT^A01^ADT_A01|M-3|P|2.5\r",
            Charset::Ascii,
        )
        .expect("the header lexes")
        .message;
        let stamped = Stamped {
            control_id: String::from("ACK-3"),
            timestamp: String::from("20261025120000+0000"),
        };
        let answer = acknowledge(
            &message,
            Code::Reject,
            &[Fault::anywhere(Condition::Application, "refused")],
            &stamped,
        );
        assert_eq!(answer.code, Code::Reject);
        assert_eq!(
            answer.text,
            "MSH|^~\\&|PIX|NORTH|ADT|EAST|20261025120000+0000||ACK^A01^ACK|ACK-3|P|2.5\rMSA|AR|M-3\rERR|||207^Application error^HL70357|E||||refused\r"
        );
    }
}
