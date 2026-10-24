// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The documents the FerroPIX server writes and its operator console reads,
//! as serde types and nothing else, so a field renamed on one side fails the
//! other's build (`docs/architecture.md` §2).
//!
//! [`health`] holds the readiness document of `GET {base}/health/readiness`
//! and the dependency report of `GET {base}/operator/dependencies`. No
//! specification governs these documents: our own design, after
//! FerroBRIDGE's operator API crate.

pub mod health;
