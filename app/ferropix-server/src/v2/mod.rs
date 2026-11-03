// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The HL7 v2 messages written and read below the lanes that use them.
//!
//! They are the PID segment both ways ([`pid`]), the segment writer
//! ([`mod@write`]), the stamp of a message ([`stamped`]), and ITI-10's ADT^A31
//! with its acknowledgement ([`notification`]).
//!
//! The HL7 v2 face (`crate::hl7v2`) answers ITI-8, ITI-9 and ITI-21 with
//! them, and the feed lane (`crate::feed`) and the outbox write and deliver
//! ITI-10 with them, so neither lane imports the other's face. No
//! specification governs the module layout: our own design.

pub mod notification;
pub mod pid;
pub mod stamped;
pub mod write;
