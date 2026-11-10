// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The keys of the family's sections that v0.0.24 renamed, each refused by
//! name for that one release.
//!
//! `[server]`, `[telemetry]`, `[auth]` and `[terminology.credentials]` are
//! the sections of `ferrohealth-server`, `ferrohealth-telemetry` and
//! `ferrohealth-smart`, which v0.0.24 takes on the lines that write every
//! span of time and size as `ferrohealth-config`'s units and gather the
//! outbound TLS keys under one `tls` table. Those crates name none of their
//! old keys, so v0.0.24, the release in which an operator first meets the
//! rename, names them here: [`Shared`] reads the sources once before the
//! typed load, and [`Shared::removals`] names every old key they set. No
//! specification governs this: our own design.

use ferrohealth_config::removed::{Removals, Removed, Retired};
use serde::Deserialize;

use super::SINCE;

/// `[server]` and `[server.tls]`.
pub mod server {
    use super::{Retired, SINCE};

    /// `server.request_timeout_ms`.
    pub const REQUEST_TIMEOUT_MS: Retired = Retired::replaced(
        "server.request_timeout_ms",
        "server.request_timeout, a duration such as `30s`",
        SINCE,
    );
    /// `server.header_read_timeout_ms`.
    pub const HEADER_READ_TIMEOUT_MS: Retired = Retired::replaced(
        "server.header_read_timeout_ms",
        "server.header_read_timeout, a duration such as `10s`",
        SINCE,
    );
    /// `server.idle_timeout_ms`.
    pub const IDLE_TIMEOUT_MS: Retired = Retired::replaced(
        "server.idle_timeout_ms",
        "server.idle_timeout, a duration such as `60s`",
        SINCE,
    );
    /// `server.body_limit_bytes`.
    pub const BODY_LIMIT_BYTES: Retired = Retired::replaced(
        "server.body_limit_bytes",
        "server.body_limit, a size such as `1MiB`",
        SINCE,
    );
    /// `server.drain_delay_ms`.
    pub const DRAIN_DELAY_MS: Retired = Retired::replaced(
        "server.drain_delay_ms",
        "server.drain_delay, a duration such as `1s`",
        SINCE,
    );
    /// `server.shutdown_timeout_ms`.
    pub const SHUTDOWN_TIMEOUT_MS: Retired = Retired::replaced(
        "server.shutdown_timeout_ms",
        "server.shutdown_timeout, a duration such as `5s`",
        SINCE,
    );
    /// `server.retry_after_s`.
    pub const RETRY_AFTER_S: Retired = Retired::replaced(
        "server.retry_after_s",
        "server.retry_after, a duration such as `1s`",
        SINCE,
    );
    /// `server.compression_min_bytes`.
    pub const COMPRESSION_MIN_BYTES: Retired = Retired::replaced(
        "server.compression_min_bytes",
        "server.compression_min_size, a size such as `1KiB`",
        SINCE,
    );
    /// `server.tls.reload_interval_s`.
    pub const TLS_RELOAD_INTERVAL_S: Retired = Retired::replaced(
        "server.tls.reload_interval_s",
        "server.tls.reload_interval, a duration such as `1h`",
        SINCE,
    );
}

/// `[telemetry.otlp]`.
pub mod telemetry {
    use super::{Retired, SINCE};

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
}

/// `[auth]`, each `[[auth.issuer]]` and the OAuth tables.
pub mod auth {
    use super::{Retired, SINCE};

    /// `auth.ca_file`.
    pub const CA_FILE: Retired = Retired::replaced(
        "auth.ca_file",
        "auth.tls.trust_roots_file, the same path",
        SINCE,
    );
    /// `auth.basic.cache_ttl_seconds`.
    pub const BASIC_CACHE_TTL_SECONDS: Retired = Retired::replaced(
        "auth.basic.cache_ttl_seconds",
        "auth.basic.cache_ttl, a duration such as `5min`",
        SINCE,
    );
    /// `auth.dpop.max_age_seconds`.
    pub const DPOP_MAX_AGE_SECONDS: Retired = Retired::replaced(
        "auth.dpop.max_age_seconds",
        "auth.dpop.max_age, a duration such as `5min`",
        SINCE,
    );
    /// `auth.dpop.clock_skew_seconds`.
    pub const DPOP_CLOCK_SKEW_SECONDS: Retired = Retired::replaced(
        "auth.dpop.clock_skew_seconds",
        "auth.dpop.clock_skew, a duration such as `60s`",
        SINCE,
    );
    /// `clock_skew_seconds` of an `[[auth.issuer]]` entry.
    pub const CLOCK_SKEW_SECONDS: Retired = Retired::replaced(
        "clock_skew_seconds",
        "clock_skew, a duration such as `60s`",
        SINCE,
    );
    /// `jwks_max_age_seconds` of an `[[auth.issuer]]` entry.
    pub const JWKS_MAX_AGE_SECONDS: Retired = Retired::replaced(
        "jwks_max_age_seconds",
        "jwks_max_age, a duration such as `1h`",
        SINCE,
    );
    /// `jwks_refresh_interval_seconds` of an `[[auth.issuer]]` entry.
    pub const JWKS_REFRESH_INTERVAL_SECONDS: Retired = Retired::replaced(
        "jwks_refresh_interval_seconds",
        "jwks_refresh_interval, a duration such as `30s`",
        SINCE,
    );
    /// `timeout_ms` of an `[[auth.issuer]]` entry.
    pub const TIMEOUT_MS: Retired =
        Retired::replaced("timeout_ms", "timeout, a duration such as `5s`", SINCE);
    /// `introspection.cache_max_age_seconds` of an `[[auth.issuer]]` entry.
    pub const INTROSPECTION_CACHE_MAX_AGE_SECONDS: Retired = Retired::replaced(
        "introspection.cache_max_age_seconds",
        "introspection.cache_max_age, a duration such as `60s`",
        SINCE,
    );
}

/// The keys of an OAuth client table: `[terminology.credentials.oauth]` and
/// an issuer's `introspection.client` and `introspection.bearer`.
pub mod oauth {
    use super::{Retired, SINCE};

    /// `refresh_margin_ms`.
    pub const REFRESH_MARGIN_MS: Retired = Retired::replaced(
        "refresh_margin_ms",
        "refresh_margin, a duration such as `30s`",
        SINCE,
    );
    /// `timeout_ms`.
    pub const TIMEOUT_MS: Retired =
        Retired::replaced("timeout_ms", "timeout, a duration such as `10s`", SINCE);
    /// `ca_file`.
    pub const CA_FILE: Retired =
        Retired::replaced("ca_file", "tls.trust_roots_file, the same path", SINCE);
    /// `client_certificate_file`.
    pub const CLIENT_CERTIFICATE_FILE: Retired = Retired::replaced(
        "client_certificate_file",
        "tls.client_identity_file, one PEM holding the certificate chain and its key",
        SINCE,
    );
    /// `client_key_file`.
    pub const CLIENT_KEY_FILE: Retired = Retired::replaced(
        "client_key_file",
        "tls.client_identity_file, one PEM holding the certificate chain and its key",
        SINCE,
    );
    /// `previous_key_overlap_ms`.
    pub const PREVIOUS_KEY_OVERLAP_MS: Retired = Retired::replaced(
        "previous_key_overlap_ms",
        "previous_key_overlap, a duration such as `1h`",
        SINCE,
    );
}

/// Every key declared here.
pub const ALL: [Retired; 26] = [
    server::REQUEST_TIMEOUT_MS,
    server::HEADER_READ_TIMEOUT_MS,
    server::IDLE_TIMEOUT_MS,
    server::BODY_LIMIT_BYTES,
    server::DRAIN_DELAY_MS,
    server::SHUTDOWN_TIMEOUT_MS,
    server::RETRY_AFTER_S,
    server::COMPRESSION_MIN_BYTES,
    server::TLS_RELOAD_INTERVAL_S,
    telemetry::OTLP_TRUST_ROOTS_FILE,
    telemetry::OTLP_CLIENT_IDENTITY_FILE,
    auth::CA_FILE,
    auth::BASIC_CACHE_TTL_SECONDS,
    auth::DPOP_MAX_AGE_SECONDS,
    auth::DPOP_CLOCK_SKEW_SECONDS,
    auth::CLOCK_SKEW_SECONDS,
    auth::JWKS_MAX_AGE_SECONDS,
    auth::JWKS_REFRESH_INTERVAL_SECONDS,
    auth::TIMEOUT_MS,
    auth::INTROSPECTION_CACHE_MAX_AGE_SECONDS,
    oauth::REFRESH_MARGIN_MS,
    oauth::TIMEOUT_MS,
    oauth::CA_FILE,
    oauth::CLIENT_CERTIFICATE_FILE,
    oauth::CLIENT_KEY_FILE,
    oauth::PREVIOUS_KEY_OVERLAP_MS,
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
    server: Server,
    telemetry: Telemetry,
    auth: Auth,
    terminology: Terminology,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Server {
    request_timeout_ms: Removed,
    header_read_timeout_ms: Removed,
    idle_timeout_ms: Removed,
    body_limit_bytes: Removed,
    drain_delay_ms: Removed,
    shutdown_timeout_ms: Removed,
    retry_after_s: Removed,
    compression_min_bytes: Removed,
    tls: ServerTls,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ServerTls {
    reload_interval_s: Removed,
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

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Auth {
    ca_file: Removed,
    issuer: Vec<Issuer>,
    basic: Basic,
    dpop: Dpop,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Issuer {
    clock_skew_seconds: Removed,
    jwks_max_age_seconds: Removed,
    jwks_refresh_interval_seconds: Removed,
    timeout_ms: Removed,
    introspection: Introspection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Introspection {
    cache_max_age_seconds: Removed,
    client: Oauth,
    bearer: Oauth,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Basic {
    cache_ttl_seconds: Removed,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Dpop {
    max_age_seconds: Removed,
    clock_skew_seconds: Removed,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Terminology {
    credentials: Credentials,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Credentials {
    oauth: Oauth,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Oauth {
    refresh_margin_ms: Removed,
    timeout_ms: Removed,
    ca_file: Removed,
    client_certificate_file: Removed,
    client_key_file: Removed,
    previous_key_overlap_ms: Removed,
}

impl Oauth {
    /// Gathers the renamed keys of the OAuth table at `table`.
    fn removals(&self, found: &mut Removals, table: &str) {
        found.check_under(self.refresh_margin_ms, &oauth::REFRESH_MARGIN_MS, table);
        found.check_under(self.timeout_ms, &oauth::TIMEOUT_MS, table);
        found.check_under(self.ca_file, &oauth::CA_FILE, table);
        found.check_under(
            self.client_certificate_file,
            &oauth::CLIENT_CERTIFICATE_FILE,
            table,
        );
        found.check_under(self.client_key_file, &oauth::CLIENT_KEY_FILE, table);
        found.check_under(
            self.previous_key_overlap_ms,
            &oauth::PREVIOUS_KEY_OVERLAP_MS,
            table,
        );
    }
}

impl Shared {
    /// Returns the renamed keys the sources set.
    #[must_use]
    pub fn removals(&self) -> Removals {
        let mut found = Removals::new();
        let server = &self.server;
        found.check(server.request_timeout_ms, &server::REQUEST_TIMEOUT_MS);
        found.check(
            server.header_read_timeout_ms,
            &server::HEADER_READ_TIMEOUT_MS,
        );
        found.check(server.idle_timeout_ms, &server::IDLE_TIMEOUT_MS);
        found.check(server.body_limit_bytes, &server::BODY_LIMIT_BYTES);
        found.check(server.drain_delay_ms, &server::DRAIN_DELAY_MS);
        found.check(server.shutdown_timeout_ms, &server::SHUTDOWN_TIMEOUT_MS);
        found.check(server.retry_after_s, &server::RETRY_AFTER_S);
        found.check(server.compression_min_bytes, &server::COMPRESSION_MIN_BYTES);
        found.check(server.tls.reload_interval_s, &server::TLS_RELOAD_INTERVAL_S);
        let otlp = &self.telemetry.otlp;
        found.check(otlp.trust_roots_file, &telemetry::OTLP_TRUST_ROOTS_FILE);
        found.check(
            otlp.client_identity_file,
            &telemetry::OTLP_CLIENT_IDENTITY_FILE,
        );
        let auth = &self.auth;
        found.check(auth.ca_file, &auth::CA_FILE);
        found.check(auth.basic.cache_ttl_seconds, &auth::BASIC_CACHE_TTL_SECONDS);
        found.check(auth.dpop.max_age_seconds, &auth::DPOP_MAX_AGE_SECONDS);
        found.check(auth.dpop.clock_skew_seconds, &auth::DPOP_CLOCK_SKEW_SECONDS);
        for (index, issuer) in auth.issuer.iter().enumerate() {
            let table = format!("auth.issuer[{index}]");
            found.check_under(issuer.clock_skew_seconds, &auth::CLOCK_SKEW_SECONDS, &table);
            found.check_under(
                issuer.jwks_max_age_seconds,
                &auth::JWKS_MAX_AGE_SECONDS,
                &table,
            );
            found.check_under(
                issuer.jwks_refresh_interval_seconds,
                &auth::JWKS_REFRESH_INTERVAL_SECONDS,
                &table,
            );
            found.check_under(issuer.timeout_ms, &auth::TIMEOUT_MS, &table);
            let introspection = &issuer.introspection;
            found.check_under(
                introspection.cache_max_age_seconds,
                &auth::INTROSPECTION_CACHE_MAX_AGE_SECONDS,
                &table,
            );
            introspection
                .client
                .removals(&mut found, &format!("{table}.introspection.client"));
            introspection
                .bearer
                .removals(&mut found, &format!("{table}.introspection.bearer"));
        }
        self.terminology
            .credentials
            .oauth
            .removals(&mut found, "terminology.credentials.oauth");
        found
    }
}
