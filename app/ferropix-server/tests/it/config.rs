// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Every section of the configuration tree loads, and every refusal the
//! tree names is its own case.

use std::collections::BTreeMap;

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
    assert_eq!("/pix", settings.server.base_path);
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
    let text =
        format!("{FULL}\n[[auth.basic.users]]\nuser = \"east-lab\"\npassword_hash = \"{PHC}\"\n");
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

[server]
listen = "127.0.0.1:8080"
public_base_url = "http://localhost:8080"

[[auth.basic.users]]
user = "dev-source"
password_hash = "{PHC}"
scopes = ["system/Patient.cud"]

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
        !error.to_string().contains("dev-source"),
        "the refusal names the key and never the value"
    );
}

#[test]
fn an_empty_configuration_names_the_first_missing_key() {
    let error = Config::from_sources(None, &BTreeMap::new())
        .expect("the defaults read")
        .resolve()
        .expect_err("the public base URL has no default");
    assert!(
        matches!(&error, Error::Missing { key } if key == "server.public_base_url"),
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
