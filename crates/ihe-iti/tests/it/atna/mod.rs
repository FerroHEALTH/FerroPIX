// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ATNA, ITI-20 Record Audit Event: the spool a sender stores into when it
//! cannot reach its repository (ITI TF-2 §3.20.4.1.1), and the forwarder
//! that delivers from it.

mod forwarder;
mod spool;

use std::time::{Duration, Instant};

use ihe_iti::atna::forwarder::{Forwarder, Status};
use secrecy::SecretSlice;

/// A synthetic message, `text` as bytes.
fn message(text: &str) -> SecretSlice<u8> {
    SecretSlice::from(text.as_bytes().to_vec())
}

/// Waits until `done` holds of the forwarder's status or `deadline` passes,
/// and returns the last status.
pub(crate) async fn until(
    forwarder: &Forwarder,
    deadline: Instant,
    done: impl Fn(&Status) -> bool,
) -> Status {
    loop {
        let status = forwarder.status();
        if done(&status) || Instant::now() >= deadline {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
