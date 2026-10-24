// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The documents the FerroPIX server writes and its operator console reads,
//! as serde types and nothing else, so a field renamed on one side fails the
//! other's build (`docs/architecture.md` §2).
//!
//! [`health`] holds the readiness document of `GET {base}/health/readiness`
//! and the dependency report of `GET {base}/operator/dependencies`;
//! [`overview`] the dashboard; [`search`] the two searches; [`person`] the
//! person with its identifiers, records, history and pseudonyms; [`reveal`]
//! the reveal of one of them; [`domain`] the identifier domains and their
//! writes; [`matching`] the matching configuration in effect; [`actions`]
//! the audit trail; and [`patient`] the values several of them share. No
//! specification governs these documents: our own design, after
//! FerroBRIDGE's operator API crate.

pub mod actions;
pub mod domain;
pub mod health;
pub mod matching;
pub mod overview;
pub mod patient;
pub mod person;
pub mod reveal;
pub mod search;
