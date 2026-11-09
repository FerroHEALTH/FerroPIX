// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! What the configuration tests share: the secret files a case writes, and a
//! configuration with every section set.

use std::collections::BTreeMap;
use std::path::PathBuf;

use tempfile::TempDir;

/// A password made up for the tests, which no rendering may show.
pub(crate) const PASSWORD: &str = "Qz7-test-password";

/// A synthetic argon2id PHC string, the one `ferrohealth-smart`'s own tests
/// use.
pub(crate) const PHC: &str = "$argon2id$v=19$m=19456,t=2,p=1$c3ludGhldGljLXNhbHQtMTY$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// A sealing key made for the tests: the base64 of 32 ASCII bytes, which no
/// rendering may show.
pub(crate) const DATA_KEY: &str = "UXo3LWEtc2VhbGluZy1rZXktZm9yLXRoZS10ZXN0cyE=";

/// The two secret files of `[db]`, in a directory removed on drop.
pub(crate) struct Secrets {
    /// The directory, held so it lives as long as the case.
    pub(crate) _directory: TempDir,
    /// The file holding the connection URL.
    pub(crate) url: PathBuf,
    /// The file holding the sealing key.
    pub(crate) key: PathBuf,
}

impl Secrets {
    /// Writes both files.
    pub(crate) fn new() -> Self {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let url = directory.path().join("db-url");
        std::fs::write(
            &url,
            format!("postgres://ferropix:{PASSWORD}@db.invalid:5432/pix?sslmode=require\n"),
        )
        .expect("the URL file is written");
        let key = directory.path().join("data-key");
        std::fs::write(&key, format!("{DATA_KEY}\n")).expect("the key file is written");
        Self {
            _directory: directory,
            url,
            key,
        }
    }

    /// Returns the overrides that point `[db]` at both files.
    pub(crate) fn environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                String::from("FERROPIX__DB__URL_FILE"),
                self.url.display().to_string(),
            ),
            (
                String::from("FERROPIX__DB__DATA_KEY_FILE"),
                self.key.display().to_string(),
            ),
        ])
    }
}

/// A production configuration with every section of the tree set.
pub(crate) const FULL: &str = r#"
profile = "production"
public_base_url = "https://pix.example.org/pix"

[server]
listen = "0.0.0.0:8080"
base_path = "/pix"
body_limit = "2MiB"
request_timeout = "20s"
drain_delay = "1s"
shutdown_timeout = "5s"

[telemetry]
filter = "info,ferropix_server=debug"
format = "json"

[telemetry.otlp]
endpoint = "https://collector.example.org:4318"

[db]
pool_size = 20
migrate = false

[auth]
realm = "ferropix"
product_scope = "__ferropix.steward"
capabilities = ["client-confidential-asymmetric"]
scopes_supported = ["system/Patient.rs"]

[[auth.issuer]]
issuer = "https://idp.example.org/realms/pix"
kind = "persons"
profile = "rfc9068"
audience = ["https://pix.example.org/pix/fhir"]
algorithms = ["PS256"]
clock_skew = "10s"
jwks_max_age = "1min"
jwks_refresh_interval = "10s"
jwks_file = ""
timeout = "5s"
role_claims = ["roles"]
first_party_clients = ["ferropix-console"]

[[trust.issuer]]
issuer = "https://idp.example.org/realms/pix"
clients = ["east-lab", "west-lab"]

[[domain]]
system = "urn:oid:2.999.1.1"
name = "East hospital"
kind = "local"
source_client_id = "east-lab"
sensitive = false

[[domain]]
system = "https://example.org/fhir/sid/national"
name = "A national number"
kind = "authoritative"
source_client_id = "west-lab"

[matching]
link_at = 0.97
review_at = 0.85
possible_at = 0.65
auto_link = false
blocking_keys = ["birth_year_family_phonetic", "shared_identifier"]

[matching.fields.family_name]
m = 0.95
u = 0.01

[pixm]
enabled = true
remove_patient = true

[pdqm]
enabled = true
deprecated = "omit"

[pmir]
enabled = true

[pmir.feed]
max_attempts = 5
initial_backoff = "500ms"
max_backoff = "1min"

[hl7v2]
enabled = true
listen = "0.0.0.0:2575"
default_charset = "UNICODE UTF-8"
frame_limit = "64KiB"
idle_timeout = "1min"
frame_timeout = "10s"

[[hl7v2.senders]]
application = "LAB"
facility = "EAST"
domain = "urn:oid:2.999.1.1"

[terminology]
base_url = "https://tx.example.org/fhir/r4"
timeout = "5s"

[terminology.retry]
max_attempts = 2
initial_backoff = "100ms"
max_backoff = "1s"

[xcpd]
enabled = true
home_community_id = "urn:oid:2.999.60.1"
device = "2.999.60.2"
domains = ["urn:oid:2.999.1.1"]
initiating_communities = ["urn:oid:2.999.70.1"]
reply_endpoints = ["https://ig.example.org/xcpd/"]
delivery_attempts = 3

[outbox]
retention = "14d"
prune_batch = 200

[audit]
url = "https://arr.example.org/fhir"
spool_dir = "/var/lib/ferropix/audit"
spool_max_size = "1MiB"
spool_max_events = 1000
spool_write_timeout = "1s"
timeout = "3s"
retry_max = "30s"

[operator]
enabled = true
admin_scope = "__ferropix.admin"

[usage_report]
enabled = true
endpoint = "https://report.ferropulse.eu/v1/report"
state_dir = "/var/lib/ferropix"
deployment = "compose"
"#;

/// Returns the lifecycle of a process that has booted and serves.
pub(crate) fn serving() -> ferrohealth_health::lifecycle::Lifecycle {
    let lifecycle = ferrohealth_health::lifecycle::Lifecycle::default();
    lifecycle.booted();
    lifecycle
}
