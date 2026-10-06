// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! BALP: the `AuditEvent` each audited client records, held to the vendored
//! audit profile of its transaction, the BALP pattern beneath it and the
//! profile's own example; the ATX: FHIR Feed that sends it (the `RESTful`
//! ATNA supplement, ITI TF-2 §3.20.4.2); and the hygiene of the patient
//! identifier a record carries.

mod described;
mod feed;
mod hygiene;
#[cfg(feature = "mcsd")]
mod mcsd;
#[cfg(feature = "pdqm")]
mod pdqm;
#[cfg(feature = "pixm")]
mod pixm;
#[cfg(feature = "pmir")]
mod pmir;
#[cfg(any(feature = "pixm", feature = "pdqm", feature = "mcsd", feature = "pmir"))]
mod profile;

use ihe_iti::balp::{NetworkAddress, Observer};

/// The system the tests record as.
pub(crate) fn observer() -> Observer {
    Observer {
        source_id: "gateway.example.org".to_owned(),
        site: Some("example site".to_owned()),
        host: NetworkAddress::host("gateway.example.org"),
    }
}
