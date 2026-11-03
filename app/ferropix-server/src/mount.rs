// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The paths under `{base}` the server mounts its faces at, below every
//! module that routes, guards or renders an answer by them.
//!
//! No specification governs this: our own design.

/// The FHIR face's path under `{base}`.
pub const FHIR: &str = "/fhir";

/// The operator API's path under `{base}`.
pub const OPERATOR: &str = "/operator";

/// The XCPD Responding Gateway's path under `{base}`.
pub const XCPD: &str = "/xcpd";
