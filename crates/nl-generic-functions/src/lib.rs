// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Netherlands Generic Functions for data exchange (the Nuts
//! `nl-generic-functions-ig`), one feature per function:
//!
//! - `nvi`: GF-Localization through the national index (NVI).
//! - `mitz`: GF-Consent through Mitz.
//! - `lrza`: GF-Addressing through the national address book (LRZa).
//! - `nuts-auth`: GF-Authentication on the Nuts profile.
//!
//! The implementation guide is published at
//! <https://build.fhir.org/ig/nuts-foundation/nl-generic-functions-ig/>. The
//! crate depends on no application. The function modules land with their
//! FerroFED issues, following `docs/architecture.md` section 6.
#![doc(test(attr(deny(warnings))))]

#[cfg(feature = "lrza")]
pub mod lrza;
#[cfg(feature = "mitz")]
pub mod mitz;
#[cfg(feature = "nuts-auth")]
pub mod nuts_auth;
#[cfg(feature = "nvi")]
pub mod nvi;
