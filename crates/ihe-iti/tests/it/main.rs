// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The integration tests of `ihe-iti`, one binary with a module per profile.
//!
//! The modules carry `#[cfg(test)]`, which an integration test always has, so
//! the test-scoped relaxations of `clippy.toml` reach their helpers too.

#[cfg(test)]
#[cfg(feature = "atna")]
mod atna;
#[cfg(test)]
#[cfg(feature = "balp")]
mod balp;
#[cfg(test)]
mod features;
#[cfg(test)]
#[cfg(feature = "mcsd")]
mod mcsd;
#[cfg(test)]
#[cfg(feature = "pdqm")]
mod pdqm;
#[cfg(test)]
#[cfg(feature = "pixm")]
mod pixm;
#[cfg(test)]
#[cfg(feature = "pmir")]
mod pmir;
#[cfg(test)]
#[cfg(any(
    feature = "atna",
    feature = "mcsd",
    feature = "pdqm",
    feature = "pixm",
    feature = "xcpd"
))]
mod timing;
#[cfg(test)]
#[cfg(feature = "xcpd")]
mod xcpd;
