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
//! writes; [`pseudonym`] the pseudonymisation domains, the mint and the
//! reverse resolution; [`source`] the systems allowed to feed; [`matching`] the matching
//! configuration in effect and [`configuration`] its settings with their
//! origins; [`actions`]
//! the audit trail; [`patient`] the values several of them share; and
//! [`problem`] the RFC 9457 problem details a refusal carries. No
//! specification governs these documents: our own design, after
//! FerroBRIDGE's operator API crate.

pub mod actions;
pub mod candidate;
pub mod configuration;
pub mod domain;
pub mod health;
pub mod matching;
pub mod merge;
pub mod overview;
pub mod patient;
pub mod person;
pub mod problem;
pub mod pseudonym;
pub mod reveal;
pub mod search;
pub mod source;
