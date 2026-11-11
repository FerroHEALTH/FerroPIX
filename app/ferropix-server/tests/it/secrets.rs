// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! No rendering of the configuration or of its settings shows a secret.

use std::collections::BTreeMap;

use ferrohealth_secret::REDACTED;
use ferrohealth_secret::secret::Secret;
use ferrohealth_secret::secret_url::SecretUrl;
use ferropix_server::config::Config;

use crate::support::{DATA_KEY, FULL, PASSWORD, Secrets, issuer_tables};

#[test]
fn the_secret_types_render_three_stars_through_debug_display_and_serialize() {
    let secret = Secret::new(PASSWORD);
    assert_eq!(REDACTED, secret.to_string());
    assert_eq!(format!("{REDACTED:?}"), format!("{secret:?}"));
    assert_eq!(
        format!("\"{REDACTED}\""),
        serde_json::to_string(&secret).expect("a secret serializes")
    );
    let url = SecretUrl::new(format!(
        "postgres://ferropix:{PASSWORD}@db.invalid:5432/pix?password={PASSWORD}"
    ));
    let shown = "postgres://***@db.invalid:5432/pix?***";
    assert_eq!(shown, url.to_string());
    assert_eq!(format!("{shown:?}"), format!("{url:?}"));
    assert_eq!(
        format!("\"{shown}\""),
        serde_json::to_string(&url).expect("a URL serializes")
    );
}

#[test]
fn a_configuration_written_back_shows_no_inline_secret() {
    let text = format!(
        "profile = \"development\"\n[db]\nurl = \"postgres://ferropix:{PASSWORD}@db.invalid/pix\"\n"
    );
    let config = Config::from_sources(Some(&text), &BTreeMap::new()).expect("the tree reads");
    let written = toml::to_string(&config).expect("the tree writes");
    assert!(!written.contains(PASSWORD), "{written}");
    assert!(
        written.contains("postgres://***@db.invalid/pix"),
        "{written}"
    );
    assert!(!format!("{config:?}").contains(PASSWORD));
}

#[test]
fn a_configuration_written_back_shows_no_issuer_userinfo() {
    let text = FULL.replacen(
        "issuer = \"https://idp.example.org/realms/pix\"",
        &format!("issuer = \"https://pix:{PASSWORD}@idp.example.org/realms/pix\""),
        1,
    );
    let config = Config::from_sources(Some(&text), &BTreeMap::new()).expect("the tree reads");
    let written = toml::to_string(&config).expect("the tree writes");
    assert!(!written.contains(PASSWORD), "{written}");
    assert!(!format!("{config:?}").contains(PASSWORD));
    let refused = config
        .resolve()
        .expect_err("an issuer with userinfo is refused (RFC 8414 §2)");
    assert!(!refused.to_string().contains(PASSWORD), "{refused}");
    assert!(!format!("{refused:?}").contains(PASSWORD));
}

/// The four `[auth]` tables that name an issuer hold it as a `SecretUrl`
/// too: its userinfo reaches neither the written-back tree, the `Debug` of
/// the configuration nor the refusal (RFC 3986 §3.2.1).
#[test]
fn a_table_naming_an_issuer_shows_no_issuer_userinfo() {
    let secrets = Secrets::new();
    for (host, table) in issuer_tables() {
        let text = format!("{FULL}{table}");
        let config =
            Config::from_sources(Some(&text), &secrets.environment()).expect("the tree reads");
        let written = toml::to_string(&config).expect("the tree writes");
        assert!(!written.contains(PASSWORD), "{host}: {written}");
        assert!(
            written.contains(&format!("https://***@{host}/realms/pix")),
            "{host}: the issuer renders masked: {written}"
        );
        assert!(!format!("{config:?}").contains(PASSWORD), "{host}");
        let refused = config
            .resolve()
            .expect_err("a table naming an issuer [auth] does not trust is refused");
        assert!(!refused.to_string().contains(PASSWORD), "{host}: {refused}");
        assert!(!format!("{refused:?}").contains(PASSWORD), "{host}");
    }
}

#[test]
fn the_resolved_settings_show_neither_the_url_password_nor_the_key() {
    let secrets = Secrets::new();
    let settings = Config::from_sources(Some(FULL), &secrets.environment())
        .expect("the tree reads")
        .resolve()
        .expect("the full configuration resolves");
    let rendered = format!("{settings:?}");
    assert!(!rendered.contains(PASSWORD), "the URL password leaked");
    assert!(!rendered.contains(DATA_KEY), "the sealing key leaked");
    assert_eq!(
        "postgres://***@db.invalid:5432/pix?***",
        settings.db.url.to_string()
    );
    assert!(settings.db.url.expose().contains(PASSWORD));
}
