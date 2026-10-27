// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The integration tests of the operator console, one binary with a module
//! per subject.
#![cfg(not(target_arch = "wasm32"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::indexing_slicing,
    reason = "test assertions"
)]

mod boundary;
mod captures;
mod config;
mod filled;
mod pages;
mod preferences;
mod pseudonyms;
mod queue;
mod refresh;
mod run;
mod server;
mod session;
mod sign_in;
mod sign_out;
mod support;
