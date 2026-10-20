// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Netherlands Generic Functions for data exchange (the Nuts
//! `nl-generic-functions-ig`, package `fhir.nl.gf` 0.3.0), one feature per
//! function:
//!
//! - `nvi`: GF-Localization through the national index (NVI): which care
//!   providers hold data for a patient, asked on a pseudonymised BSN.
//! - `mitz`: GF-Consent through Mitz: the closed authorization question,
//!   whether a data holder may make a patient's data available to a data
//!   user, asked on the BSN.
//! - `lrza`: GF-Addressing through the national address book (LRZa): the
//!   care provider identifier (URA) of an NL-GF `Organization`.
//! - `nuts-auth`: GF-Authentication on the Nuts profile: a `DPoP`-bound
//!   access token for a Verifiable Presentation of the holder's credentials
//!   (Nuts RFC021), its authorization server held to the RFC 8414 checks of
//!   the `oauth-server-metadata` crate.
//! - `authorizer`: the authorizer a data user supplies to authenticate each
//!   request to a Generic Function (the IG's GFI-005), with no FHIR model;
//!   `nvi` turns it on.
//!
//! The identifier systems the functions share, GF-Identification, are in
//! the `identification` module, built whenever `nvi`, `lrza` or `mitz` is on.
//!
//! The implementation guide is published at
//! <https://build.fhir.org/ig/nuts-foundation/nl-generic-functions-ig/>. The
//! crate depends on no application.
#![doc(test(attr(deny(warnings))))]

#[cfg(feature = "authorizer")]
pub mod authorizer;
#[cfg(any(feature = "nvi", feature = "lrza", feature = "mitz"))]
pub mod identification;
#[cfg(feature = "lrza")]
pub mod lrza;
#[cfg(feature = "mitz")]
pub mod mitz;
#[cfg(feature = "nuts-auth")]
pub mod nuts_auth;
#[cfg(feature = "nvi")]
pub mod nvi;
