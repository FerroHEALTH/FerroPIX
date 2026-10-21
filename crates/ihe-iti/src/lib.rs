// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The IHE IT Infrastructure (ITI) identity profiles a patient index, a
//! gateway or any other client binds, one feature per profile:
//!
//! - `pixm`: Patient Identifier Cross-reference for Mobile, ITI-83.
//! - `pdqm`: Patient Demographics Query for Mobile, ITI-78 and ITI-119.
//! - `mcsd`: Mobile Care Services Discovery, ITI-90 and ITI-91.
//! - `pmir`: Patient Master Identity Registry, ITI-93 and ITI-94.
//! - `xcpd`: Cross-Community Patient Discovery, ITI-55, the one profile on
//!   SOAP 1.2 with HL7 v3 and SAML XUA.
//! - `balp`: the BALP `AuditEvent` of ITI-83, ITI-78, ITI-119, ITI-90,
//!   ITI-91, ITI-93 and ITI-94 with `pixm`, `pdqm`, `mcsd` or `pmir`.
//! - `atna`: the DICOM audit message of ITI-55 with `xcpd`, with no FHIR
//!   model.
//!
//! The audit trail the records go to (ATNA ITI-20 and its FHIR Feed) is the
//! `ihe-atna` crate.
//!
//! The PIXm, PDQm, mCSD and PMIR clients authenticate with a credential the
//! HTTP client carries, or with an access token incorporated in each request
//! by an authorizer the caller supplies (module `authorizer`, IUA ITI-72).
//!
//! A PIXm, PDQm or XCPD exchange names whom it is made for, a user or the
//! client's own system (`ihe_atna::user`), and its audit record names that
//! user.
//!
//! The profiles are published at <https://profiles.ihe.net/ITI/>. The crate
//! depends on no application: it is the profiles' transactions as Rust, for
//! any caller. Its home is FerroHEALTH/FerroPIX, the FerroHEALTH Master
//! Patient Index, which adds the server sides as features.
#![doc(test(attr(deny(warnings))))]

#[cfg(any(feature = "pixm", feature = "pdqm", feature = "mcsd", feature = "pmir"))]
pub mod authorizer;
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
#[cfg(any(feature = "pixm", feature = "pdqm", feature = "mcsd", feature = "pmir"))]
mod search;
#[cfg(feature = "xcpd")]
pub mod xcpd;
