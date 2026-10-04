// SPDX-FileCopyrightText: Vernum Projecten B.V.
// SPDX-License-Identifier: BUSL-1.1

//! ATNA, the audit trail of a Secure Node or Secure Application: ITI-20
//! Record Audit Event (ITI TF-2 §3.20).
//!
//! - [`message`]: the DICOM PS3.15 Annex A.5 `AuditMessage` an event is
//!   recorded as.
//! - [`syslog`]: the RFC 5424 syslog message that carries it, with the PRI and
//!   MSGID §3.20.4.1.2 fixes, and its RFC 5425 octet-counted frame.
//! - [`repository`]: the connection to an Audit Record Repository, TLS (RFC
//!   5425) unless a development caller asks otherwise.
//! - [`spool`]: the bounded local store ITI-20 requires of a sender that
//!   cannot reach its repository (§3.20.4.1.1): one file per message on disk,
//!   fsynced before it counts as stored, or a queue in memory for
//!   development.
//! - [`forwarder`]: the sender that writes every message to the spool first
//!   and delivers it from there, in order.
//!
//! A message names whatever its event names, a patient identifier included,
//! so no part of this module logs a message or renders one in `Debug`.

pub mod forwarder;
pub mod message;
pub mod repository;
pub mod spool;
pub mod syslog;
