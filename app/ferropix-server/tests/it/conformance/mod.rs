// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The server's conformance corpora, behind `FERROPIX_E2E`: the PDQm answers
//! held to the MyHealth@EU NCP API Patient-minimal profile, the EHDS
//! identifier types every served identifier carries, and the Xt-EHR D8.2
//! patient match assertions. Each records its verdicts through
//! `ferropix_conformance`, and `scripts/checks/conformance.sh` runs them
//! after it fetches the validator's package closure.
//!
//! No specification governs the instrument: our own design.

mod identifier_types;
mod ncp;
mod patient_match;
