// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Test support for the FerroPIX suites, consumed as a path-only
//! dev-dependency so it never ships.
//!
//! [`containers`] starts PostgreSQL 18 and the stub OpenID Provider behind
//! the `FERROPIX_E2E` gate, each image pinned by digest; [`synthetic`] holds
//! the synthetic BSN range with its eleven-proof, the synthetic identifier
//! domains and the synthetic names; [`fixtures`] loads the `Patient`,
//! `Parameters` and `Bundle` documents of each transaction and decodes them
//! into the FHIR R4 model; [`guard`] refuses a fixture that carries a BSN
//! outside the synthetic range or a name outside the synthetic set; and
//! [`pins`] reads the container image rows of `docs/VERSIONS.md`.
//!
//! No specification governs the test topology: our own design, after
//! FerroFED's and FerroBRIDGE's testkits.
#![doc(test(attr(deny(warnings))))]

pub mod containers;
pub mod fixtures;
pub mod guard;
pub mod pins;
pub mod synthetic;
