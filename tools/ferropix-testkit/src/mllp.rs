// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The MLLP sender the HL7 v2 face is driven with.
//!
//! A message travels as one MLLP Release 1 frame: the start block `0x0B`,
//! the message bytes, the end block `0x1C` and a carriage return `0x0D` (HL7
//! Version 3 Standard, Transport Specification, MLLP Release 1, Block
//! format). [`send`] frames a message and reads the answer's frame back;
//! [`exchange`] writes any bytes, a malformed or a stalled frame included,
//! and reads what comes back until the answer's frame ends or the listener
//! closes the connection.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

/// The start block.
pub const START_BLOCK: u8 = 0x0B;

/// The end block.
pub const END_BLOCK: u8 = 0x1C;

/// The carriage return that closes a frame.
pub const CARRIAGE_RETURN: u8 = 0x0D;

/// How long the sender waits for an answer before it gives up.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// Why an exchange failed.
#[derive(Debug, thiserror::Error)]
pub enum SendError {
    /// The connection failed.
    #[error("the MLLP connection failed")]
    Io(#[from] std::io::Error),
    /// No answer arrived within [`ANSWER_TIMEOUT`].
    #[error("no answer arrived in time")]
    Timeout(#[source] tokio::time::error::Elapsed),
    /// The listener closed the connection without an answer.
    #[error("the listener closed the connection without an answer")]
    Closed,
    /// The answer is no UTF-8 text.
    #[error("the answer is no UTF-8 text")]
    Text(#[source] std::string::FromUtf8Error),
}

/// Returns `message` framed.
#[must_use]
pub fn frame(message: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(message.len().saturating_add(3));
    bytes.push(START_BLOCK);
    bytes.extend_from_slice(message.as_bytes());
    bytes.push(END_BLOCK);
    bytes.push(CARRIAGE_RETURN);
    bytes
}

/// Sends `message` framed to `address` and returns the answer's message
/// text, unframed.
///
/// # Errors
/// Returns [`SendError`] when the connection fails, no answer arrives in
/// time, the listener closes without one, or the answer is no UTF-8 text.
pub async fn send(address: SocketAddr, message: &str) -> Result<String, SendError> {
    let answer = exchange(address, &frame(message))
        .await?
        .ok_or(SendError::Closed)?;
    String::from_utf8(answer).map_err(SendError::Text)
}

/// Writes `bytes` to `address` as they are and returns the message bytes of
/// the answer's frame, or `None` when the listener closes the connection
/// without one.
///
/// # Errors
/// Returns [`SendError::Io`] when the connection fails and
/// [`SendError::Timeout`] when neither an answer nor a close arrives in time.
pub async fn exchange(address: SocketAddr, bytes: &[u8]) -> Result<Option<Vec<u8>>, SendError> {
    let mut stream = TcpStream::connect(address).await?;
    stream.write_all(bytes).await?;
    tokio::time::timeout(ANSWER_TIMEOUT, read_answer(&mut stream))
        .await
        .map_err(SendError::Timeout)?
}

/// Reads one answer's frame from `stream`.
async fn read_answer(stream: &mut TcpStream) -> Result<Option<Vec<u8>>, SendError> {
    let mut read = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Ok(None);
        }
        read.extend_from_slice(chunk.get(..count).unwrap_or_default());
        if let Some(end) = read
            .windows(2)
            .position(|pair| pair == [END_BLOCK, CARRIAGE_RETURN])
        {
            let start = read
                .iter()
                .position(|byte| *byte == START_BLOCK)
                .map_or(0, |at| at.saturating_add(1));
            return Ok(Some(read.get(start..end).unwrap_or_default().to_vec()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CARRIAGE_RETURN, END_BLOCK, START_BLOCK, frame};

    #[test]
    fn a_message_is_framed_in_the_block_characters() {
        let framed = frame("MSH|^~\\&");
        assert_eq!(framed.first(), Some(&START_BLOCK));
        assert!(framed.ends_with(&[END_BLOCK, CARRIAGE_RETURN]));
    }
}
