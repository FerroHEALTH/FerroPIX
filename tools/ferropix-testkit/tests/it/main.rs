// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The testkit's integration suite: the fixtures decode and pass the guard,
//! the guard refuses what it must, every image is pinned in the matrix, and
//! the containers start behind `FERROPIX_E2E`.
//!
//! The modules carry `#[cfg(test)]`, which an integration test always has, so
//! the test-scoped relaxations of `clippy.toml` reach their helpers too.

#[cfg(test)]
mod e2e;
#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod guard;
#[cfg(test)]
mod pins;
