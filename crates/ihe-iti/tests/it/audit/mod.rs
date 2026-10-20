// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The `AuditEvent` each audited client records, held to the vendored audit
//! profile of its transaction, the BALP pattern beneath it and the profile's
//! own example. The audit trail that sends it is `ihe-atna`'s, tested there.

#[cfg(feature = "mcsd")]
mod mcsd;
#[cfg(feature = "pdqm")]
mod pdqm;
#[cfg(feature = "pixm")]
mod pixm;
#[cfg(feature = "pmir")]
mod pmir;
mod profile;

use ihe_atna::balp::{NetworkAddress, Observer};

/// The system the tests record as.
pub(crate) fn observer() -> Observer {
    Observer {
        source_id: "gateway.example.org".to_owned(),
        site: Some("example site".to_owned()),
        host: NetworkAddress::host("gateway.example.org"),
    }
}
