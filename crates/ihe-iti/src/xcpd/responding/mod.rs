// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The Responding Gateway side of ITI-55, Cross Gateway Patient Discovery
//! (ITI TF-2 §3.55, Revision 20.1).
//!
//! [`query::read`] reads a `PRPA_IN201305UV02` from its SOAP 1.2 envelope
//! into a [`query::ReceivedQuery`] in any of the three modes of §3.55.1;
//! [`answer::envelope`] writes the `PRPA_IN201306UV02` of the case the
//! gateway decided ([`answer::Outcome`], §3.55.4.2.3), and
//! [`answer::deferred_refused`] the acknowledgement of a Deferred request a
//! gateway without the Deferred Response Option refuses (§3.55.4.1.3).
//! [`answer::fault`] is the SOAP fault of a request that is no ITI-55 query.
//! [`audit::RespondingAuditEvent`] is the gateway's audit message
//! (§3.55.5.1.2).
//!
//! Matching is the gateway's own (§3.55.4.1.3.1); the Asynchronous Web
//! Services Exchange (Appendix V.5) is the deployment's to carry, on the
//! `ReplyTo` the query names. The query's values are directly identifying:
//! every type that holds one redacts it in `Debug`, and no refusal carries a
//! value.

pub mod answer;
pub mod audit;
pub mod query;
