// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The probabilistic matching of `docs/architecture.md` §5.2: pure, with no
//! store.
//!
//! [`subject`] reads a record version's demographics into the form the
//! scorer compares, [`normalise`] gives each value that form, [`compare`]
//! holds the comparator of each field, [`blocking`] the keys a pair must
//! share to be scored, [`scorer`] the Fellegi-Sunter weights, the score,
//! the grade and the evidence, [`conflict`] the hard conflicts that hold a
//! deterministic link for review, and [`estimate`] the `u` a field's value
//! frequencies give. The same inputs give the same score, grade
//! and evidence, bit for bit.
//!
//! No specification governs the matching: PIXm places it outside IHE (PIXm
//! 3.1.0 §2:3.104.4.1.3) and PDQm asks only for a score and a grade (PDQm
//! 3.2.0 §2:3.119.4.2.2.4). Our own design.

pub mod blocking;
pub mod compare;
pub mod conflict;
pub mod estimate;
pub mod normalise;
pub mod scorer;
pub mod subject;
