// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Patient Identity Registry side of PMIR 1.6.0 (feature `pmir-server`).
//!
//! The Registry of ITI-93, Mobile Patient Identity Feed, and of ITI-94,
//! Subscribe to Patient Updates, typed and with no store and no HTTP server:
//!
//! - [`feed::FeedMessage`] reads a feed message in FHIR JSON or XML, held to
//!   the PMIR Bundle, `MessageHeader`, history Bundle and merged Patient
//!   profiles (§2:3.93.4.1.2.1 to §2:3.93.4.1.2.4), into typed creates,
//!   updates, deletes and merges with their Patients, and
//!   [`feed::response`] writes the response message (§2:3.93.4.2.2).
//! - [`subscription::RequestedSubscription`] reads a `Subscription` against
//!   the PMIR Subscription profile with one of the four criteria
//!   (§2:3.94.4.1.2.1, §2:3.94.4.1.2.1.1), and
//!   [`subscription::Criteria::matches`] says whether a Patient falls under
//!   it.
//! - [`notification::message`] builds the ITI-93 message the Registry sends a
//!   subscriber for a change, which the subscriber side of
//!   [`crate::pmir::feed::Feed`] reads.
//!
//! The store, the master identities and the router are the caller's. Every
//! Patient sits behind a type whose `Debug` leaves it out, and no type here
//! has `Display`.

pub mod feed;
pub mod notification;
pub mod subscription;
