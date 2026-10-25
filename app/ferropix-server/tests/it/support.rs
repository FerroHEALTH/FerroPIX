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

[server]
listen = "0.0.0.0:8080"
base_path = "/pix"
public_base_url = "https://pix.example.org/pix"
body_limit_bytes = 2097152
request_timeout_ms = 20000
drain_timeout_ms = 5000

[telemetry]
filter = "info,ferropix_server=debug"
format = "json"
otlp_endpoint = "https://collector.example.org:4318"

[db]
pool_size = 20
migrate = false

[auth]
audience = ["https://pix.example.org/pix/fhir"]
algorithms = ["RS256", "ES256"]
token_profile = "rfc9068"
clock_skew_seconds = 30
jwks_max_age_seconds = 300
jwks_refresh_interval_seconds = 30
jwks_file = ""
timeout_ms = 5000
challenge_realm = "ferropix"
steward_scope = "__ferropix.steward"
admin_scope = "__ferropix.admin"

[[auth.issuer]]
issuer = "https://idp.example.org/realms/pix"
clients = ["east-lab", "west-lab"]
audience = ["https://pix.example.org/pix/fhir"]
algorithms = ["PS256"]
token_profile = "jwt"
clock_skew_seconds = 10
jwks_max_age_seconds = 60
jwks_refresh_interval_seconds = 10
jwks_file = ""

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
initial_backoff_ms = 500
max_backoff_ms = 60000

[hl7v2]
enabled = true
listen = "0.0.0.0:2575"
default_charset = "UNICODE UTF-8"
frame_limit_bytes = 65536
idle_timeout_ms = 60000
frame_timeout_ms = 10000

[[hl7v2.senders]]
application = "LAB"
facility = "EAST"
domain = "urn:oid:2.999.1.1"

[xcpd]
enabled = true
home_community_id = "urn:oid:2.999.60.1"
device = "2.999.60.2"
domains = ["urn:oid:2.999.1.1"]
initiating_communities = ["urn:oid:2.999.70.1"]
reply_endpoints = ["https://ig.example.org/xcpd/"]
delivery_attempts = 3

[audit]
url = "https://arr.example.org/fhir"
spool_dir = "/var/lib/ferropix/audit"
spool_max_bytes = 1048576
spool_max_events = 1000
spool_write_timeout_ms = 1000
timeout_ms = 3000
retry_max_ms = 30000

[operator]
enabled = true

[usage_report]
enabled = true
endpoint = "https://report.ferropulse.eu/v1/report"
state_dir = "/var/lib/ferropix"
deployment = "compose"
"#;
