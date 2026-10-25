// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The control id and the time an answer of the face carries of its own.

use ferrobridge_hl7v2::ack::Stamp;

/// An answer's own MSH-10 and MSH-7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamped {
    /// MSH-10: twenty hexadecimal digits of a random UUID.
    pub control_id: String,
    /// MSH-7: the instant in UTC, as a v2 `DTM` with its offset.
    pub timestamp: String,
}

impl Stamped {
    /// Returns the stamp of an answer written now.
    #[must_use]
    pub fn now() -> Self {
        let uuid = uuid::Uuid::new_v4().simple().to_string();
        // NOTE: no specification governs this: our own design; twenty characters
        // fit the MSH-10 of every 2.x version the face reads, as FerroBRIDGE's does.
        let control_id = uuid.get(..20).map_or_else(|| uuid.clone(), str::to_owned);
        let timestamp = format!("{}+0000", jiff::Timestamp::now().strftime("%Y%m%d%H%M%S"));
        Self {
            control_id,
            timestamp,
        }
    }

    /// Returns the stamp `ferrobridge-hl7v2`'s acknowledgment renders.
    #[must_use]
    pub fn stamp(&self) -> Stamp<'_> {
        Stamp {
            control_id: &self.control_id,
            timestamp: &self.timestamp,
        }
    }
}
