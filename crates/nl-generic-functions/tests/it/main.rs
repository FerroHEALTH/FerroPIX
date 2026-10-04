// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The integration tests of `nl-generic-functions`, one binary with a module
//! per function.
//!
//! The modules carry `#[cfg(test)]`, which an integration test always has, so
//! the test-scoped relaxations of `clippy.toml` reach their helpers too.

#[cfg(test)]
#[cfg(any(feature = "nvi", feature = "lrza"))]
mod ig;
#[cfg(test)]
#[cfg(feature = "lrza")]
mod lrza;
#[cfg(test)]
#[cfg(feature = "nvi")]
mod nvi;
