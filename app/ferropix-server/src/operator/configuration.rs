// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! `GET {base}/operator/configuration`: the matching configuration, read
//! only, each setting with where its value came from.
//!
//! FerroPIX keeps no configuration store: the matching settings are read
//! from the file at start, so the console draws them in file mode, every tab
//! read-only with the file named once, and a change is made in the file and
//! takes effect at the next start (FerroUI 0.3.7 `design/COMPONENTS.md`
//! rule 13). The sections are the thresholds, the field weights, the
//! blocking keys and the deterministic rules, which are built in
//! (`docs/architecture.md` §5.1). No specification governs the API: our
//! own design.

use axum::response::Response;
use ferropix_operator_api::configuration::{Configuration, Mode, Origin, Section, Setting};

use crate::config::origin::{self, Origins};
use crate::config::section::matching::Matching;
use crate::matching::scorer::{FIELDS, default_weights};
use crate::operator::{Operator, matching, wire};

/// The deterministic rules, built in: their names and what they hold.
pub const RULES: [(&str, &str); 3] = [
    (
        "deterministic.authoritative_identifier",
        "two records carrying one identifier of an authoritative domain are one patient",
    ),
    (
        "deterministic.hard_conflict",
        "a birth year or a stated gender that differs turns the link into a candidate",
    ),
    (
        "deterministic.local_domains",
        "an identifier of a local domain never links records across domains",
    ),
];

/// Answers the configuration in effect.
#[must_use]
pub fn answer(operator: &Operator) -> Response {
    wire::answer(&document(&operator.matching, &operator.configured.origins))
}

/// Returns the configuration document of `matching`, its settings marked by
/// `origins`.
#[must_use]
pub fn document(matching: &Matching, origins: &Origins) -> Configuration {
    let setting = |key: &str, value: String| Setting {
        key: key.to_owned(),
        value,
        origin: origin_of(origins.origin(key)),
    };
    let thresholds = vec![
        setting("matching.link_at", matching.link_at.to_string()),
        setting("matching.review_at", matching.review_at.to_string()),
        setting("matching.possible_at", matching.possible_at.to_string()),
        setting("matching.auto_link", matching.auto_link.to_string()),
    ];
    let mut weights = Vec::new();
    for field in FIELDS {
        let held = matching
            .fields
            .get(&field)
            .copied()
            .unwrap_or_else(|| default_weights(field));
        let key = format!("matching.fields.{}", field.key());
        weights.push(setting(&format!("{key}.m"), held.m.to_string()));
        weights.push(setting(&format!("{key}.u"), held.u.to_string()));
    }
    let keys: Vec<String> = matching::document(matching)
        .blocking_keys
        .iter()
        .map(|key| format!("\"{key}\""))
        .collect();
    let blocking = vec![setting(
        "matching.blocking_keys",
        format!("[{}]", keys.join(", ")),
    )];
    let rules = RULES
        .iter()
        .map(|(key, value)| Setting {
            key: (*key).to_owned(),
            value: (*value).to_owned(),
            origin: Origin::BuiltIn,
        })
        .collect();
    Configuration {
        mode: Mode::File,
        file: origins.file().map(str::to_owned),
        sections: vec![
            section("thresholds", thresholds),
            section("weights", weights),
            section("blocking", blocking),
            section("rules", rules),
        ],
    }
}

/// One section.
fn section(name: &str, settings: Vec<Setting>) -> Section {
    Section {
        name: name.to_owned(),
        settings,
    }
}

/// The wire origin of a configuration origin.
const fn origin_of(origin: origin::Origin) -> Origin {
    match origin {
        origin::Origin::File => Origin::File,
        origin::Origin::Environment => Origin::Environment,
        origin::Origin::Default => Origin::Default,
    }
}

#[cfg(test)]
mod tests {
    use ferropix_operator_api::configuration::{Mode, Origin};

    use super::document;
    use crate::config::origin::Origins;
    use crate::config::section::matching::Matching;

    #[test]
    fn every_setting_names_its_origin_and_the_rules_are_built_in() {
        let file: toml::Table = toml::from_str("[matching]\nlink_at = 0.95\n").expect("TOML");
        let origins = Origins::of(&file, &[String::from("matching.auto_link")])
            .with_file(Some(String::from("ferropix.toml")));
        let read = document(&Matching::default(), &origins);
        assert_eq!(read.mode, Mode::File);
        assert_eq!(read.file.as_deref(), Some("ferropix.toml"));
        let names: Vec<&str> = read.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["thresholds", "weights", "blocking", "rules"]);
        let origin = |key: &str| {
            read.sections
                .iter()
                .flat_map(|section| &section.settings)
                .find(|setting| setting.key == key)
                .map(|setting| setting.origin)
        };
        assert_eq!(origin("matching.link_at"), Some(Origin::File));
        assert_eq!(origin("matching.auto_link"), Some(Origin::Environment));
        assert_eq!(origin("matching.review_at"), Some(Origin::Default));
        assert_eq!(origin("matching.fields.gender.m"), Some(Origin::Default));
        assert_eq!(
            origin("deterministic.authoritative_identifier"),
            Some(Origin::BuiltIn)
        );
    }
}
