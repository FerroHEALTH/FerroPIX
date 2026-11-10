// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Every section of the configuration tree loads, and every refusal the
//! tree names is its own case.

use std::collections::BTreeMap;

use ferrohealth_server::cli::ConfigArgs;
use ferropix_server::config::Config;
use ferropix_server::config::error::Error;
use ferropix_server::config::section::domain::DomainKind;
use ferropix_server::config::section::profiles::Deprecated;
use nl_generic_functions::identification::{BSN_SYSTEMS, PSEUDO_BSN_SYSTEM};

use crate::support::{FULL, PASSWORD, PHC, Secrets};

/// Reads `text` with the secret files of `secrets` and resolves it.
fn resolve(
    text: &str,
    secrets: &Secrets,
) -> Result<ferropix_server::config::settings::Settings, Error> {
    Config::from_sources(Some(text), &secrets.environment())?.resolve()
}

#[test]
fn every_section_loads_and_resolves() {
    let secrets = Secrets::new();
    let settings = resolve(FULL, &secrets).expect("the full configuration resolves");
    assert_eq!("/pix", settings.server.base_path.as_str());
    assert_eq!(
        "https://pix.example.org/pix",
        settings.public_base_url.as_str()
    );
    assert_eq!(1000, settings.server.drain_delay.as_millis());
    assert_eq!(5000, settings.server.shutdown_timeout.as_millis());
    assert_eq!(8080, settings.server.listen.port());
    assert!(
        settings
            .telemetry
            .otlp
            .as_ref()
            .is_some_and(|otlp| !otlp.endpoint.is_empty())
    );
    assert_eq!(20, settings.db.pool_size);
    assert!(!settings.db.migrate);
    assert_eq!(1, settings.auth.resolved.issuers.len());
    assert!(settings.auth.admits("west-lab"));
    assert_eq!(2, settings.domains.len());
    assert_eq!(
        Some(DomainKind::Authoritative),
        settings.domains.get(1).map(|domain| domain.kind)
    );
    assert!(
        settings
            .domains
            .get(1)
            .is_some_and(|domain| domain.sensitive),
        "a domain is sealed unless it says otherwise"
    );
    assert_eq!(1, settings.matching.fields.len());
    assert!(settings.pixm.remove_patient);
    assert_eq!(Deprecated::Omit, settings.pdqm.deprecated);
    assert_eq!(5, settings.pmir.feed.max_attempts);
    assert_eq!(
        Some(1),
        settings.hl7v2.as_ref().map(|hl7v2| hl7v2.senders.len())
    );
    assert_eq!(
        Some("urn:oid:2.999.60.1"),
        settings
            .xcpd
            .as_ref()
            .map(|xcpd| xcpd.community.to_string())
            .as_deref()
    );
    assert_eq!(200, settings.outbox.batch);
    assert!(settings.audit.url.is_some());
    assert!(settings.operator.enabled);
    assert!(settings.usage_report.enabled);
}

#[test]
fn an_unknown_key_is_refused_in_every_table_and_named() {
    let secrets = Secrets::new();
    for (table, anchor) in [
        ("top level", "profile = \"production\"\n"),
        ("[server]", "[server]\n"),
        ("[telemetry]", "[telemetry]\n"),
        ("[db]", "[db]\n"),
        ("[auth]", "[auth]\n"),
        ("[[auth.issuer]]", "[[auth.issuer]]\n"),
        ("[[trust.issuer]]", "[[trust.issuer]]\n"),
        ("[[domain]]", "[[domain]]\n"),
        ("[matching]", "[matching]\n"),
        (
            "[matching.fields.family_name]",
            "[matching.fields.family_name]\n",
        ),
        ("[pixm]", "[pixm]\n"),
        ("[pdqm]", "[pdqm]\n"),
        ("[pmir]", "[pmir]\n"),
        ("[pmir.feed]", "[pmir.feed]\n"),
        ("[hl7v2]", "[hl7v2]\n"),
        ("[[hl7v2.senders]]", "[[hl7v2.senders]]\n"),
        ("[terminology]", "[terminology]\n"),
        ("[terminology.retry]", "[terminology.retry]\n"),
        ("[xcpd]", "[xcpd]\n"),
        ("[outbox]", "[outbox]\n"),
        ("[audit]", "[audit]\n"),
        ("[operator]", "[operator]\n"),
        ("[usage_report]", "[usage_report]\n"),
    ] {
        let text = FULL.replacen(anchor, &format!("{anchor}misspelt_key = 1\n"), 1);
        assert_ne!(FULL, text, "{table} is in the full configuration");
        let error = resolve(&text, &secrets).expect_err("an unknown key is refused");
        assert!(
            matches!(error, Error::Load(_)),
            "{table}: a load refusal, not {error:?}"
        );
        assert!(
            error.to_string().contains("misspelt_key"),
            "{table}: the refusal names the key: {error}"
        );
    }
    for (table, text) in [
        (
            "[dev]",
            "profile = \"development\"\n[dev]\nmisspelt_key = 1\n",
        ),
        ("[auth.basic]", "[auth.basic]\nmisspelt_key = 1\n"),
    ] {
        let error = Config::from_sources(Some(text), &BTreeMap::new())
            .expect_err("an unknown key is refused");
        assert!(
            error.to_string().contains("misspelt_key"),
            "{table}: {error}"
        );
    }
}

#[test]
fn a_bare_secret_is_refused_under_production_where_its_file_is_required() {
    let secrets = Secrets::new();
    let mut environment = secrets.environment();
    environment.remove("FERROPIX__DB__URL_FILE");
    environment.insert(
        String::from("FERROPIX__DB__URL"),
        format!("postgres://ferropix:{PASSWORD}@db.invalid/pix"),
    );
    let error = Config::from_sources(Some(FULL), &environment)
        .expect("the tree reads")
        .resolve()
        .expect_err("production takes the URL from url_file");
    assert!(
        matches!(&error, Error::InlineSecret { key } if key == "db.url"),
        "{error:?}"
    );
    assert!(!error.to_string().contains(PASSWORD), "{error}");
    let development = FULL
        .replacen("profile = \"production\"", "profile = \"development\"", 1)
        .replacen(
            "[db]\n",
            &format!("[db]\nurl = \"postgres://ferropix:{PASSWORD}@db.invalid/pix\"\n"),
            1,
        );
    let mut environment = secrets.environment();
    environment.remove("FERROPIX__DB__URL_FILE");
    Config::from_sources(Some(&development), &environment)
        .expect("the tree reads")
        .resolve()
        .expect("development admits the bare URL");
}

#[test]
fn a_domain_whose_source_client_no_issuer_admits_is_refused() {
    let secrets = Secrets::new();
    let text = FULL.replacen(
        "source_client_id = \"west-lab\"",
        "source_client_id = \"north-lab\"",
        1,
    );
    let error = resolve(&text, &secrets).expect_err("no issuer lists north-lab");
    assert!(
        matches!(&error, Error::ClientNotAdmitted { index: 1, client } if client == "north-lab"),
        "{error:?}"
    );
}

#[test]
fn a_system_seeded_twice_is_refused() {
    let secrets = Secrets::new();
    let text = FULL.replacen(
        "system = \"https://example.org/fhir/sid/national\"",
        "system = \"urn:oid:2.999.1.1\"",
        1,
    );
    let error = resolve(&text, &secrets).expect_err("one entry per system");
    assert!(
        matches!(&error, Error::DuplicateSystem { index: 1, system } if system == "urn:oid:2.999.1.1"),
        "{error:?}"
    );
}

#[test]
fn a_domain_without_its_kind_or_with_a_system_that_is_no_uri_is_refused() {
    let secrets = Secrets::new();
    let kindless = FULL.replacen("kind = \"local\"\n", "", 1);
    assert!(matches!(
        resolve(&kindless, &secrets),
        Err(Error::Missing { key }) if key == "domain[0].kind"
    ));
    let bad = FULL.replacen("urn:oid:2.999.1.1\"\nname", "east\"\nname", 1);
    assert!(matches!(
        resolve(&bad, &secrets),
        Err(Error::Value { key, .. }) if key == "domain[0].system"
    ));
}

#[test]
fn the_dev_table_is_refused_under_production() {
    let secrets = Secrets::new();
    let text = format!("{FULL}\n[dev]\nloopback = false\n");
    let error = resolve(&text, &secrets).expect_err("production refuses [dev]");
    assert!(matches!(error, Error::DevUnderProduction), "{error:?}");
}

#[test]
fn a_basic_user_is_refused_under_production() {
    let secrets = Secrets::new();
    let text = format!(
        "{FULL}\n[[auth.basic.users]]\nuser = \"dev-steward\"\npassword_hash = \"{PHC}\"\nscopes = [\"__ferropix.steward\"]\n"
    );
    let error = resolve(&text, &secrets).expect_err("production refuses Basic");
    assert!(
        matches!(&error, Error::DevelopmentOnly { key, .. } if key == "auth.basic"),
        "{error:?}"
    );
}

#[test]
fn the_dev_profile_runs_on_loopback_with_basic_users_and_refuses_another_address() {
    let secrets = Secrets::new();
    let development = format!(
        r#"
profile = "development"
public_base_url = "http://localhost:8080"

[server]
listen = "127.0.0.1:8080"

[[auth.issuer]]
issuer = "http://127.0.0.1:8180/dev"
kind = "machines"
audience = ["http://localhost:8080/fhir"]

[[trust.issuer]]
issuer = "http://127.0.0.1:8180/dev"
clients = ["dev-source"]

[[auth.basic.users]]
user = "dev-steward"
password_hash = "{PHC}"
scopes = ["__ferropix.steward"]

[[domain]]
system = "urn:oid:2.999.1.1"
name = "Development hospital"
kind = "local"
source_client_id = "dev-source"

[dev]
"#
    );
    let settings = resolve(&development, &secrets).expect("the development profile resolves");
    assert!(settings.dev.is_some_and(|dev| dev.loopback));
    let wide = development.replacen("127.0.0.1:8080", "0.0.0.0:8080", 1);
    let error = resolve(&wide, &secrets).expect_err("[dev] loopback holds by default");
    assert!(
        matches!(&error, Error::NotLoopback { key } if key == "server.listen"),
        "{error:?}"
    );
    let opened = wide.replacen("[dev]\n", "[dev]\nloopback = false\n", 1);
    resolve(&opened, &secrets).expect("loopback = false admits any address");
    let undeclared = development.replacen("[dev]\n", "", 1);
    let error = resolve(&undeclared, &secrets).expect_err("Basic users ride with [dev] only");
    assert!(
        matches!(&error, Error::DevTableRequired { key } if key == "auth.basic"),
        "{error:?}"
    );
    assert!(
        !error.to_string().contains("dev-steward"),
        "the refusal names the key and never the value"
    );
}

#[test]
fn a_basic_user_granted_a_system_scope_fails_at_boot_naming_the_user() {
    let secrets = Secrets::new();
    let text = format!(
        r#"
profile = "development"
public_base_url = "http://localhost:8080"

[server]
listen = "127.0.0.1:8080"

[[auth.basic.users]]
user = "dev-steward"
password_hash = "{PHC}"
scopes = ["__ferropix.steward"]

[[auth.basic.users]]
user = "east-lab"
password_hash = "{PHC}"
scopes = ["system/Patient.cud"]

[dev]
"#
    );
    let error = resolve(&text, &secrets).expect_err("a Basic user is a person");
    assert!(
        matches!(&error, Error::BasicSystemScope { index: 1, user } if user == "east-lab"),
        "{error:?}"
    );
    assert!(error.to_string().contains("east-lab"), "{error}");
}

#[test]
fn a_basic_user_is_never_the_source_of_a_domain() {
    let secrets = Secrets::new();
    let text = format!(
        r#"
profile = "development"
public_base_url = "http://localhost:8080"

[server]
listen = "127.0.0.1:8080"

[[auth.basic.users]]
user = "dev-source"
password_hash = "{PHC}"
scopes = ["__ferropix.steward"]

[[domain]]
system = "urn:oid:2.999.1.1"
name = "Development hospital"
kind = "local"
source_client_id = "dev-source"

[dev]
"#
    );
    let error = resolve(&text, &secrets).expect_err("no issuer lists dev-source");
    assert!(
        matches!(&error, Error::ClientNotAdmitted { index: 0, client } if client == "dev-source"),
        "{error:?}"
    );
}

/// Walks the cause chain of `error` into one line, as `config check` prints
/// it.
fn chain(error: &Error) -> String {
    let mut text = error.to_string();
    let mut cause = std::error::Error::source(error);
    while let Some(inner) = cause {
        text.push_str(": ");
        text.push_str(&inner.to_string());
        cause = inner.source();
    }
    text
}

#[test]
fn every_key_v0_0_24_renamed_is_refused_naming_its_replacement() {
    let secrets = Secrets::new();
    let text = FULL
        .replacen("[outbox]\n", "[outbox]\nretention_days = 14\n", 1)
        .replacen("[audit]\n", "[audit]\ntimeout_ms = 3000\n", 1)
        .replacen(
            "[terminology.retry]\n",
            "[terminology.retry]\nmax_backoff_ms = 1000\n",
            1,
        )
        .replacen(
            "[hl7v2]\n",
            "[[hl7v2.consumers]]\nname = \"east\"\naddress = \"consumer.example.org:2575\"\napplication = \"PIX\"\nfacility = \"EAST\"\nreply_timeout_ms = 2000\n\n[hl7v2]\n",
            1,
        );
    let error = resolve(&text, &secrets).expect_err("a renamed key is refused");
    assert!(matches!(error, Error::Removed(_)), "{error:?}");
    let said = chain(&error);
    for (old, new) in [
        ("outbox.retention_days", "outbox.retention"),
        ("audit.timeout_ms", "audit.timeout"),
        (
            "terminology.retry.max_backoff_ms",
            "terminology.retry.max_backoff",
        ),
        ("hl7v2.consumers[0].reply_timeout_ms", "reply_timeout"),
    ] {
        assert!(
            said.contains(&format!("{old} was replaced by {new}")),
            "{old}: {said}"
        );
    }
}

/// Reads `text` as the file `--config` names, with the `--set` pairs of
/// `sets`, as the run path does.
fn load(text: &str, sets: &[(&str, &str)]) -> Result<(), Error> {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("ferropix.toml");
    std::fs::write(&path, text).expect("the file is written");
    let args = ConfigArgs {
        config: Some(path),
        sets: sets
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    };
    Config::load(&args).map(drop)
}

#[test]
fn every_family_key_v0_0_24_renamed_is_refused_naming_its_replacement() {
    let text = "[server]\nrequest_timeout_ms = 30000\n\n[auth]\nca_file = \"/run/ca.pem\"\n\n[[auth.issuer]]\nissuer = \"https://idp.example.org/realms/pix\"\nclock_skew_seconds = 30\n";
    let error = load(
        text,
        &[("telemetry.otlp.trust_roots_file", "/run/otlp-ca.pem")],
    )
    .expect_err("a renamed key is refused before the typed load");
    assert!(matches!(error, Error::Removed(_)), "{error:?}");
    let said = chain(&error);
    for (old, new) in [
        ("server.request_timeout_ms", "server.request_timeout"),
        ("auth.ca_file", "auth.tls.trust_roots_file"),
        ("auth.issuer[0].clock_skew_seconds", "clock_skew"),
        (
            "telemetry.otlp.trust_roots_file",
            "telemetry.otlp.tls.trust_roots_file",
        ),
    ] {
        assert!(
            said.contains(&format!("{old} was replaced by {new}")),
            "{old}: {said}"
        );
    }
}

#[test]
fn a_renamed_key_of_an_oauth_table_is_named_under_its_table() {
    let text = "[terminology.credentials.oauth]\ntimeout_ms = 5000\nclient_certificate_file = \"/run/client.pem\"\n";
    let error = load(text, &[]).expect_err("a renamed key is refused before the typed load");
    let said = chain(&error);
    for (old, new) in [
        ("terminology.credentials.oauth.timeout_ms", "timeout"),
        (
            "terminology.credentials.oauth.client_certificate_file",
            "tls.client_identity_file",
        ),
    ] {
        assert!(
            said.contains(&format!("{old} was replaced by {new}")),
            "{old}: {said}"
        );
    }
}

#[test]
fn a_file_without_a_renamed_key_reaches_the_typed_load() {
    let error = load("[server]\nlisten = \"not an address\"\n", &[])
        .expect_err("the typed load refuses the address");
    assert!(matches!(error, Error::Load(_)), "{error:?}");
}

// NOTE: `ferrohealth-smart` 0.15 dropped the named refusal of each removed key one release
// after it removed them (FerroHEALTH/FerroAUTH#388), so each fails as an unknown key.
#[test]
fn every_removed_auth_key_fails_at_boot_as_an_unknown_key() {
    let secrets = Secrets::new();
    for (key, line) in [
        (
            "audience",
            "audience = [\"https://pix.example.org/pix/fhir\"]",
        ),
        ("algorithms", "algorithms = [\"RS256\"]"),
        ("token_profile", "token_profile = \"rfc9068\""),
        ("clock_skew_seconds", "clock_skew_seconds = 30"),
        ("jwks_max_age_seconds", "jwks_max_age_seconds = 300"),
        (
            "jwks_refresh_interval_seconds",
            "jwks_refresh_interval_seconds = 30",
        ),
        ("jwks_file", "jwks_file = \"\""),
        ("timeout_ms", "timeout_ms = 5000"),
        ("role_claims", "role_claims = [\"roles\"]"),
        ("mode", "mode = \"jwt\""),
    ] {
        let text = FULL.replacen("[auth]\n", &format!("[auth]\n{line}\n"), 1);
        let error = resolve(&text, &secrets).expect_err("a section-level issuer key is removed");
        assert!(matches!(error, Error::Load(_)), "{key}: {error:?}");
        let said = chain(&error);
        assert!(said.contains(&format!("auth.{key}")), "{key}: {said}");
    }
}

#[test]
fn the_old_issuer_shapes_fail_at_boot_naming_their_key() {
    let secrets = Secrets::new();
    let issuer = "issuer = \"https://idp.example.org/realms/pix\"\n";
    let table = FULL
        .find("[[auth.issuer]]")
        .zip(FULL.find("[[trust.issuer]]"))
        .and_then(|(from, to)| FULL.get(from..to))
        .expect("the full configuration has an issuer table");
    for (case, text, names) in [
        (
            "the flat issuer",
            FULL.replacen(table, "", 1)
                .replacen("[auth]\n", &format!("[auth]\n{issuer}"), 1),
            "auth.issuer",
        ),
        (
            "token_profile on an issuer",
            FULL.replacen(
                &format!("[[auth.issuer]]\n{issuer}"),
                &format!("[[auth.issuer]]\n{issuer}token_profile = \"jwt\"\n"),
                1,
            ),
            "token_profile",
        ),
        (
            "a string scopeless subject",
            FULL.replacen(
                "[auth]\n",
                "[auth]\nscopeless_subjects = [\"https://idp.example.org/realms/pix#east-lab\"]\n",
                1,
            ),
            "auth.scopeless_subjects",
        ),
    ] {
        assert_ne!(FULL, text, "{case}");
        let error = resolve(&text, &secrets).expect_err("the old shape is refused");
        assert!(matches!(error, Error::Load(_)), "{case}: {error:?}");
        let said = chain(&error);
        assert!(said.contains(names), "{case}: {said}");
    }
}

#[test]
fn the_hl7v2_face_needs_a_terminology_server() {
    let secrets = Secrets::new();
    let text = FULL.replacen("base_url = \"https://tx.example.org/fhir/r4\"\n", "", 1);
    assert_ne!(FULL, text);
    let error = resolve(&text, &secrets).expect_err("the face translates on the server");
    assert!(
        matches!(&error, Error::Value { key, .. } if key == "terminology.base_url"),
        "{error:?}"
    );
}

#[test]
fn an_issuer_without_its_kind_fails_at_boot() {
    let secrets = Secrets::new();
    let text = FULL.replacen("kind = \"persons\"\n", "", 1);
    assert_ne!(FULL, text);
    let error = resolve(&text, &secrets).expect_err("kind has no default");
    assert!(matches!(error, Error::Auth { .. }), "{error:?}");
    let said = chain(&error);
    assert!(said.contains("kind"), "{said}");
}

#[test]
fn an_empty_configuration_names_the_first_missing_key() {
    let error = Config::from_sources(None, &BTreeMap::new())
        .expect("the defaults read")
        .resolve()
        .expect_err("the public base URL has no default");
    assert!(
        matches!(&error, Error::Missing { key } if key == "public_base_url"),
        "{error:?}"
    );
}

#[test]
fn the_quickstart_configuration_resolves_with_its_two_secret_files() {
    let secrets = Secrets::new();
    let quickstart = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("quickstart.toml"),
    )
    .expect("the quickstart file is read");
    let settings = resolve(&quickstart, &secrets).expect("the quickstart resolves");
    let systems: Vec<(&str, DomainKind, bool)> = settings
        .domains
        .iter()
        .map(|domain| (domain.system.as_str(), domain.kind, domain.sensitive))
        .collect();
    assert_eq!(
        vec![
            ("urn:oid:2.999.1.1", DomainKind::Local, true),
            (BSN_SYSTEMS[0], DomainKind::Authoritative, true),
            (PSEUDO_BSN_SYSTEM, DomainKind::Authoritative, true),
        ],
        systems,
        "the hospital, the BSN and the pseudonymised BSN, both sealed and authoritative"
    );
}
