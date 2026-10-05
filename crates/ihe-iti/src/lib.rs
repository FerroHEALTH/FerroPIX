// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The IHE IT Infrastructure (ITI) profiles a federation gateway or a patient
//! index binds, one feature per profile:
//!
//! - `pixm`: Patient Identifier Cross-reference for Mobile, ITI-83.
//! - `pdqm`: Patient Demographics Query for Mobile, ITI-78 and ITI-119.
//! - `mcsd`: Mobile Care Services Discovery, ITI-90 and ITI-91.
//! - `pmir`: Patient Master Identity Registry, ITI-93 and ITI-94.
//! - `xcpd`: Cross-Community Patient Discovery, ITI-55, the one profile on
//!   SOAP 1.2 with HL7 v3 and SAML XUA.
//! - `atna`: Audit Trail and Node Authentication, ITI-20, the DICOM audit
//!   message over syslog with TLS.
//! - `balp`: the Basic Audit Log Patterns, the FHIR `AuditEvent` the FHIR
//!   profiles audit their transactions as, sent over ITI-20's FHIR Feed; with
//!   `pixm`, `pdqm`, `mcsd` or `pmir` it also audits that profile's
//!   transactions.
//!
//! The PIXm, PDQm, mCSD and PMIR clients authenticate with a credential the
//! HTTP client carries, or with an access token incorporated in each request
//! by an authorizer the caller supplies (module `authorizer`, IUA ITI-72).
//!
//! An audited PIXm, PDQm or XCPD exchange names whom it is made for, a user
//! or the client's own system (module `user`), and its audit record names that
//! user.
//!
//! The profiles are published at <https://profiles.ihe.net/ITI/>. The crate
//! depends on no application: it is the profiles' transactions as Rust, for
//! any caller. The profile modules land with their FerroFED issues (Annex A).
#![doc(test(attr(deny(warnings))))]

#[cfg(feature = "atna")]
pub mod atna;
#[cfg(any(feature = "pixm", feature = "pdqm", feature = "mcsd", feature = "pmir"))]
pub mod authorizer;
#[cfg(feature = "balp")]
pub mod balp;
#[cfg(feature = "mcsd")]
pub mod mcsd;
#[cfg(any(feature = "pixm", feature = "pdqm", feature = "mcsd", feature = "pmir"))]
pub mod outcome;
#[cfg(feature = "pdqm")]
pub mod pdqm;
#[cfg(feature = "pixm")]
pub mod pixm;
#[cfg(feature = "pmir")]
pub mod pmir;
#[cfg(any(
    all(feature = "balp", any(feature = "pixm", feature = "pdqm")),
    feature = "xcpd"
))]
pub mod recording;
#[cfg(any(
    feature = "atna",
    feature = "pixm",
    feature = "pdqm",
    feature = "mcsd",
    feature = "pmir",
    feature = "xcpd"
))]
mod redact;
#[cfg(any(
    feature = "pixm",
    feature = "pdqm",
    feature = "mcsd",
    feature = "pmir",
    feature = "balp"
))]
mod search;
#[cfg(any(feature = "balp", feature = "pixm", feature = "pdqm", feature = "xcpd"))]
pub mod user;
#[cfg(feature = "xcpd")]
pub mod xcpd;
