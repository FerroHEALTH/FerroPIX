// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The keys that v0.0.24 renamed, each refused by name for that one release.
//!
//! v0.0.24 writes every span of time as `ferrohealth-config`'s units under a
//! key that names none, and gathers the outbound TLS keys under one `tls`
//! table. `[pix]` keeps its old key as a `Removed` field. `[session]`,
//! `[oidc]` and `[telemetry]` are the sections of `ferrohealth-session` and
//! `ferrohealth-telemetry`, which name none of their old keys, so the
//! console names them in the release in which an operator first meets the
//! rename: [`Shared`] reads the sources once before the typed load. The next
//! release deletes every refusal here (the owner's one-release rule of
//! 2026-11-09), which the test below holds through [`PREPARING`]. No
//! specification governs this: our own design.

use ferrohealth_config::removed::{Removals, Removed, Retired};
use serde::Deserialize;

/// The release that renamed the keys below.
pub const SINCE: &str = "0.0.24";

/// The release this tree prepares.
///
/// The release pull request moves the workspace version to it, and the
/// first pull request after the cut moves it to the next release, which
/// fails the test below until every refusal of the release just cut is
/// deleted.
pub const PREPARING: &str = "0.0.24";

/// `pix.timeout_ms`.
pub const PIX_TIMEOUT_MS: Retired = Retired::replaced(
    "pix.timeout_ms",
    "pix.timeout, a duration such as `30s`",
    SINCE,
);
/// `session.sign_in_timeout_s`.
pub const SESSION_SIGN_IN_TIMEOUT_S: Retired = Retired::replaced(
    "session.sign_in_timeout_s",
    "session.sign_in_timeout, a duration such as `10min`",
    SINCE,
);
/// `session.idle_timeout_s`.
pub const SESSION_IDLE_TIMEOUT_S: Retired = Retired::replaced(
    "session.idle_timeout_s",
    "session.idle_timeout, a duration such as `30min`",
    SINCE,
);
/// `session.absolute_timeout_s`.
pub const SESSION_ABSOLUTE_TIMEOUT_S: Retired = Retired::replaced(
    "session.absolute_timeout_s",
    "session.absolute_timeout, a duration such as `8h`",
    SINCE,
);
/// `oidc.timeout_s`.
pub const OIDC_TIMEOUT_S: Retired = Retired::replaced(
    "oidc.timeout_s",
    "oidc.timeout, a duration such as `10s`",
    SINCE,
);
/// `oidc.ca_file`.
pub const OIDC_CA_FILE: Retired = Retired::replaced(
    "oidc.ca_file",
    "oidc.tls.trust_roots_file, the same path",
    SINCE,
);
/// `oidc.tls.client_certificate`.
pub const OIDC_TLS_CLIENT_CERTIFICATE: Retired = Retired::replaced(
    "oidc.tls.client_certificate",
    "oidc.tls.client_identity_file, one PEM holding the certificate chain and its key",
    SINCE,
);
/// `oidc.tls.client_key`.
pub const OIDC_TLS_CLIENT_KEY: Retired = Retired::replaced(
    "oidc.tls.client_key",
    "oidc.tls.client_identity, the certificate chain and its key in one PEM",
    SINCE,
);
/// `oidc.tls.client_key_file`.
pub const OIDC_TLS_CLIENT_KEY_FILE: Retired = Retired::replaced(
    "oidc.tls.client_key_file",
    "oidc.tls.client_identity_file, one PEM holding the certificate chain and its key",
    SINCE,
);
/// `telemetry.otlp.trust_roots_file`.
pub const OTLP_TRUST_ROOTS_FILE: Retired = Retired::replaced(
    "telemetry.otlp.trust_roots_file",
    "telemetry.otlp.tls.trust_roots_file, the same path",
    SINCE,
);
/// `telemetry.otlp.client_identity_file`.
pub const OTLP_CLIENT_IDENTITY_FILE: Retired = Retired::replaced(
    "telemetry.otlp.client_identity_file",
    "telemetry.otlp.tls.client_identity_file, the same path",
    SINCE,
);

/// Every key declared here.
pub const ALL: [Retired; 11] = [
    PIX_TIMEOUT_MS,
    SESSION_SIGN_IN_TIMEOUT_S,
    SESSION_IDLE_TIMEOUT_S,
    SESSION_ABSOLUTE_TIMEOUT_S,
    OIDC_TIMEOUT_S,
    OIDC_CA_FILE,
    OIDC_TLS_CLIENT_CERTIFICATE,
    OIDC_TLS_CLIENT_KEY,
    OIDC_TLS_CLIENT_KEY_FILE,
    OTLP_TRUST_ROOTS_FILE,
    OTLP_CLIENT_IDENTITY_FILE,
];

/// The renamed keys of the family's sections, as the sources set them.
///
/// No table refuses an unknown key here: every other key is the typed
/// load's to read, and sources that do not read even as this tree are the
/// typed load's to refuse, which names the fault (no specification governs
/// this: our own design).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Shared {
    session: Session,
    oidc: Oidc,
    telemetry: Telemetry,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_field_names,
    reason = "each field is an old key as an operator wrote it"
)]
struct Session {
    sign_in_timeout_s: Removed,
    idle_timeout_s: Removed,
    absolute_timeout_s: Removed,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Oidc {
    timeout_s: Removed,
    ca_file: Removed,
    tls: OidcTls,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_field_names,
    reason = "each field is an old key as an operator wrote it"
)]
struct OidcTls {
    client_certificate: Removed,
    client_key: Removed,
    client_key_file: Removed,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Telemetry {
    otlp: Otlp,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Otlp {
    trust_roots_file: Removed,
    client_identity_file: Removed,
}

impl Shared {
    /// Returns the renamed keys the sources set.
    #[must_use]
    pub fn removals(&self) -> Removals {
        let mut found = Removals::new();
        let session = &self.session;
        found.check(session.sign_in_timeout_s, &SESSION_SIGN_IN_TIMEOUT_S);
        found.check(session.idle_timeout_s, &SESSION_IDLE_TIMEOUT_S);
        found.check(session.absolute_timeout_s, &SESSION_ABSOLUTE_TIMEOUT_S);
        let oidc = &self.oidc;
        found.check(oidc.timeout_s, &OIDC_TIMEOUT_S);
        found.check(oidc.ca_file, &OIDC_CA_FILE);
        found.check(oidc.tls.client_certificate, &OIDC_TLS_CLIENT_CERTIFICATE);
        found.check(oidc.tls.client_key, &OIDC_TLS_CLIENT_KEY);
        found.check(oidc.tls.client_key_file, &OIDC_TLS_CLIENT_KEY_FILE);
        let otlp = &self.telemetry.otlp;
        found.check(otlp.trust_roots_file, &OTLP_TRUST_ROOTS_FILE);
        found.check(otlp.client_identity_file, &OTLP_CLIENT_IDENTITY_FILE);
        found
    }
}

#[cfg(test)]
mod tests {
    use ferrohealth_config::removed::stale;

    use super::{ALL, PREPARING, Retired};

    #[test]
    fn every_refusal_lives_for_the_one_release_that_renamed_its_key() {
        let past: Vec<&str> = stale(&ALL, PREPARING)
            .into_iter()
            .map(Retired::key)
            .collect();
        assert!(
            past.is_empty(),
            "v{PREPARING} is prepared, and these refusals belong to an earlier release: delete them from src/config/retired.rs: {past:?}"
        );
    }
}
