// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The stub Patient Identifier Cross-reference Consumer ITI-10 is tested
//! against.
//!
//! It listens on a loopback port with `ferrobridge-hl7v2`'s MLLP listener,
//! keeps every message it receives in order, and answers each with the
//! original-mode acknowledgment `ferrobridge-hl7v2` renders, MSA-2 echoing
//! MSH-10 (ITI TF-2 §3.10.4.1.2: "Each message shall be acknowledged by the
//! HL7 ACK message"). It answers with the code it was started with.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use ferrobridge_hl7v2::ack::{self, Code, Header, Stamp};
use ferrobridge_hl7v2::decode::Charset;
use ferrobridge_hl7v2::mllp::{Codec, Connection, Handler, serve};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

/// The stub's handler: the messages kept, and the code each is answered
/// with.
#[derive(Debug)]
struct Answering {
    /// Every message received, as text, in order.
    received: Arc<Mutex<Vec<String>>>,
    /// The acknowledgment code.
    code: Code,
}

impl Handler for Answering {
    fn handle(
        &self,
        message: Vec<u8>,
        _connection: &Connection,
    ) -> impl Future<Output = Option<Vec<u8>>> + Send {
        std::future::ready(self.answer(&message))
    }
}

impl Answering {
    /// Keeps `message` and returns its acknowledgment.
    fn answer(&self, message: &[u8]) -> Option<Vec<u8>> {
        let text = String::from_utf8_lossy(message).into_owned();
        if let Ok(mut received) = self.received.lock() {
            received.push(text);
        }
        let header = Header::from_bytes(message)?;
        let stamp = Stamp {
            control_id: "STUB-ACK",
            timestamp: "20261026120000+0000",
        };
        ack::encoded(&header, self.code, &[], stamp, Charset::Utf8).ok()
    }
}

/// A running stub consumer, stopped when dropped.
#[derive(Debug)]
pub struct Consumer {
    /// The address it listens on.
    address: SocketAddr,
    /// Every message received, in order.
    received: Arc<Mutex<Vec<String>>>,
    /// The switch that stops the listener.
    stop: Option<oneshot::Sender<()>>,
}

impl Consumer {
    /// Starts a consumer answering every message with `code`.
    ///
    /// # Errors
    /// Returns the I/O error of a loopback port that cannot be bound.
    pub async fn start(code: Code) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let received = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(Answering {
            received: Arc::clone(&received),
            code,
        });
        let (stop, stopped) = oneshot::channel::<()>();
        drop(tokio::spawn(serve(
            listener,
            handler,
            Codec::default(),
            async {
                let _signal = stopped.await;
            },
        )));
        Ok(Self {
            address,
            received,
            stop: Some(stop),
        })
    }

    /// Returns the `host:port` the consumer listens on.
    #[must_use]
    pub fn address(&self) -> String {
        self.address.to_string()
    }

    /// Returns every message received so far, in order.
    #[must_use]
    pub fn received(&self) -> Vec<String> {
        self.received
            .lock()
            .map(|received| received.clone())
            .unwrap_or_default()
    }
}

impl Drop for Consumer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _stopped = stop.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use ferrobridge_hl7v2::ack::Code;

    use super::Consumer;
    use crate::mllp;

    #[tokio::test]
    async fn the_stub_keeps_each_message_and_answers_it_with_its_code() {
        let consumer = Consumer::start(Code::Error).await.expect("it starts");
        let message = "MSH|^~\\&|FERROPIX|NORTH|PIXCONSUMER|WEST|20261026120000||ADT^A31^ADT_A05|N-1|P|2.5\rEVN|A31|20261026120000\rPID|1||E-1^^^&2.999.1&ISO|| \rPV1||N\r";
        let address = consumer.address().parse().expect("an address");
        let answer = mllp::send(address, message).await.expect("an answer");
        assert!(answer.contains("\rMSA|AE|N-1"), "{answer}");
        assert_eq!(vec![message.to_owned()], consumer.received());
    }
}
