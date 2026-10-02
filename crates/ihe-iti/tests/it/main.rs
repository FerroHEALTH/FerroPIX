// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The integration tests of `ihe-iti`, one binary with a module per profile.
//!
//! The modules carry `#[cfg(test)]`, which an integration test always has, so
//! the test-scoped relaxations of `clippy.toml` reach their helpers too.

#[cfg(test)]
#[cfg(feature = "pdqm")]
mod pdqm;
#[cfg(test)]
#[cfg(feature = "pixm")]
mod pixm;
