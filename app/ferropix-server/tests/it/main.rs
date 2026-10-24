// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The integration tests of the FerroPIX server, one module per topic.
//!
//! The modules carry `#[cfg(test)]`, which an integration test always has, so
//! the test-scoped relaxations of `clippy.toml` reach their helpers too.

#[cfg(test)]
mod auth;
#[cfg(test)]
mod book;
#[cfg(test)]
mod cli;
#[cfg(test)]
mod config;
#[cfg(test)]
mod secrets;
#[cfg(test)]
mod serve;
#[cfg(test)]
mod support;
