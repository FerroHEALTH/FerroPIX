// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Netherlands Generic Functions for data exchange (the Nuts
//! `nl-generic-functions-ig`, package `fhir.nl.gf` 0.3.0), one feature per
//! function:
//!
//! - `nvi`: GF-Localization through the national index (NVI): which care
//!   providers hold data for a patient, asked on a pseudonymised BSN.
//! - `mitz`: GF-Consent through Mitz.
//! - `lrza`: GF-Addressing through the national address book (LRZa): the
//!   care provider identifier (URA) of an NL-GF `Organization`.
//! - `nuts-auth`: GF-Authentication on the Nuts profile.
//!
//! The identifier systems the functions share, GF-Identification, are in
//! [`identification`] whenever `nvi` or `lrza` is on.
//!
//! The implementation guide is published at
//! <https://build.fhir.org/ig/nuts-foundation/nl-generic-functions-ig/>. The
//! crate depends on no application.
#![doc(test(attr(deny(warnings))))]

#[cfg(any(feature = "nvi", feature = "lrza"))]
pub mod identification;
#[cfg(feature = "lrza")]
pub mod lrza;
#[cfg(feature = "mitz")]
pub mod mitz;
#[cfg(feature = "nuts-auth")]
pub mod nuts_auth;
#[cfg(feature = "nvi")]
pub mod nvi;
