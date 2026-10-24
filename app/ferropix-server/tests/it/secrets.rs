// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! No rendering of the configuration or of its settings shows a secret.

use std::collections::BTreeMap;

use ferropix_server::config::Config;
use ferropix_server::config::secret::{REDACTED, Secret, SecretUrl};

use crate::support::{FULL, PASSWORD, Secrets};

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
fn the_resolved_settings_show_neither_the_url_password_nor_the_key() {
    let secrets = Secrets::new();
    let settings = Config::from_sources(Some(FULL), &secrets.environment())
        .expect("the tree reads")
        .resolve()
        .expect("the full configuration resolves");
    let rendered = format!("{settings:?}");
    assert!(!rendered.contains(PASSWORD), "the URL password leaked");
    assert!(
        !rendered.contains("a-sealing-key-made-for-this-test"),
        "the sealing key leaked"
    );
    assert_eq!(
        "postgres://***@db.invalid:5432/pix?***",
        settings.db.url.to_string()
    );
    assert!(settings.db.url.expose().contains(PASSWORD));
}
