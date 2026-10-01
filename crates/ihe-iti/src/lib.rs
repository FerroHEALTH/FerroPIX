// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The IHE IT Infrastructure (ITI) profiles a federation gateway or a patient
//! index binds, one feature per profile:
//!
//! - `pixm`: Patient Identifier Cross-reference for Mobile, ITI-83.
//! - `pdqm`: Patient Demographics Query for Mobile, ITI-78.
//! - `mcsd`: Mobile Care Services Discovery, ITI-90.
//! - `pmir`: Patient Master Identity Registry, ITI-93 and ITI-104.
//! - `xcpd`: Cross-Community Patient Discovery, ITI-55, the one profile on
//!   SOAP 1.2 with HL7 v3 and SAML XUA.
//!
//! The profiles are published at <https://profiles.ihe.net/ITI/>. The crate
//! depends on no application: it is the profiles' transactions as Rust, for
//! any caller. The profile modules land with their FerroFED issues, following
//! `docs/architecture.md` section 6.
#![doc(test(attr(deny(warnings))))]

#[cfg(feature = "mcsd")]
pub mod mcsd;
#[cfg(feature = "pdqm")]
pub mod pdqm;
#[cfg(feature = "pixm")]
pub mod pixm;
#[cfg(feature = "pmir")]
pub mod pmir;
#[cfg(feature = "xcpd")]
pub mod xcpd;
