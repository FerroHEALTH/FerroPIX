// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The MLLP sender the HL7 v2 face is driven with, over
//! `ferrobridge_hl7v2::mllp`.
//!
//! [`send`] is that crate's sender with this suite's timeouts. [`exchange`]
//! writes any bytes, a malformed or a stalled frame included, and reads the
//! answer's frame back through that crate's [`Codec`], until the frame ends
//! or the listener closes the connection.

use std::net::SocketAddr;
use std::time::Duration;

use bytes::BytesMut;
use ferrobridge_hl7v2::mllp::{self, Codec, FrameError, SendTimeouts};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
use tokio_util::codec::Decoder as _;

/// How long the sender waits for a connection and for an answer before it
/// gives up.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// How many bytes one read of the connection asks for.
const READ_CHUNK: usize = 4096;

/// Why an exchange failed.
#[derive(Debug, thiserror::Error)]
pub enum SendError {
    /// The sender of `ferrobridge-hl7v2` returned no answer.
    #[error("the MLLP exchange failed")]
    Mllp(#[from] mllp::SendError),
    /// The connection failed.
    #[error("the MLLP connection failed")]
    Io(#[from] std::io::Error),
    /// The answer is no MLLP frame.
    #[error("the answer is no MLLP frame")]
    Frame(#[from] FrameError),
    /// No answer arrived within [`ANSWER_TIMEOUT`].
    #[error("no answer arrived in time")]
    Timeout(#[source] tokio::time::error::Elapsed),
    /// The answer is no UTF-8 text.
    #[error("the answer is no UTF-8 text")]
    Text(#[source] std::string::FromUtf8Error),
}

/// Sends `message` framed to `address` and returns the answer's message
/// text, unframed.
///
/// # Errors
/// Returns [`SendError::Mllp`] when `ferrobridge_hl7v2::mllp::send` returns
/// no answer, and [`SendError::Text`] when the answer is no UTF-8 text.
pub async fn send(address: SocketAddr, message: &str) -> Result<String, SendError> {
    let timeouts = SendTimeouts {
        connect: ANSWER_TIMEOUT,
        reply: ANSWER_TIMEOUT,
    };
    let answer = mllp::send(address, message.as_bytes(), Codec::default(), timeouts).await?;
    String::from_utf8(answer).map_err(SendError::Text)
}

/// Writes `bytes` to `address` as they are and returns the message bytes of
/// the answer's frame, or `None` when the listener closes the connection
/// without one.
///
/// # Errors
/// Returns [`SendError::Io`] when the connection fails,
/// [`SendError::Frame`] when the answer is no frame, and
/// [`SendError::Timeout`] when neither an answer nor a close arrives in time.
pub async fn exchange(address: SocketAddr, bytes: &[u8]) -> Result<Option<Vec<u8>>, SendError> {
    let mut stream = TcpStream::connect(address).await?;
    stream.write_all(bytes).await?;
    tokio::time::timeout(ANSWER_TIMEOUT, read_answer(&mut stream))
        .await
        .map_err(SendError::Timeout)?
}

/// Reads one answer's frame from `stream` through the codec.
async fn read_answer(stream: &mut TcpStream) -> Result<Option<Vec<u8>>, SendError> {
    let mut codec = Codec::default();
    let mut buffer = BytesMut::new();
    loop {
        if let Some(answer) = codec.decode(&mut buffer)? {
            return Ok(Some(answer));
        }
        buffer.reserve(READ_CHUNK);
        if stream.read_buf(&mut buffer).await? == 0 {
            if buffer.is_empty() {
                return Ok(None);
            }
            return Ok(codec.decode_eof(&mut buffer)?);
        }
    }
}
