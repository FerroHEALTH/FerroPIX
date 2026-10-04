// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ATNA, ITI-20 Record Audit Event: the spool a sender stores into when it
//! cannot reach its repository (ITI TF-2 §3.20.4.1.1), and the forwarder
//! that delivers from it.

mod forwarder;
mod spool;

use secrecy::SecretSlice;

/// A synthetic message, `text` as bytes.
fn message(text: &str) -> SecretSlice<u8> {
    SecretSlice::from(text.as_bytes().to_vec())
}
