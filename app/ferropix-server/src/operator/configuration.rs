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

use ferrohealth_config::provenance::{Provenance, Source};

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
    wire::answer(&document(
        &operator.matching,
        &operator.configured.provenance,
    ))
}

/// Returns the configuration document of `matching`, its settings marked by
/// `provenance`.
#[must_use]
pub fn document(matching: &Matching, provenance: &Provenance) -> Configuration {
    let setting = |key: &str, value: String| Setting {
        key: key.to_owned(),
        value,
        origin: origin_of(provenance.source(key)),
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
        file: file_name(provenance),
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

/// The name of the configuration file the load read, without its
/// directory, when it read one.
#[must_use]
pub fn file_name(provenance: &Provenance) -> Option<String> {
    provenance
        .file()
        .and_then(std::path::Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
}

/// The wire origin of the layer a key's value came from.
const fn origin_of(source: Source) -> Origin {
    match source {
        Source::File => Origin::File,
        // NOTE: no specification governs this: our own design; FerroUI draws no
        // source for a `--set` value, which overrides the start as a variable does.
        Source::Environment | Source::Set => Origin::Environment,
        Source::Default => Origin::Default,
    }
}

#[cfg(test)]
mod tests {
    use ferropix_operator_api::configuration::{Mode, Origin};

    use std::collections::BTreeMap;
    use std::path::Path;

    use super::document;
    use crate::config::section::matching::Matching;
    use crate::config::{Config, loader};

    #[test]
    fn every_setting_names_its_origin_and_the_rules_are_built_in() {
        let environment = BTreeMap::from([(
            String::from("FERROPIX__MATCHING__AUTO_LINK"),
            String::from("false"),
        )]);
        let loaded = loader()
            .expect("the namespace is valid")
            .load_sources::<Config>(
                Some("[matching]\nlink_at = 0.95\n"),
                Some(Path::new("/etc/ferropix/ferropix.toml")),
                &environment,
            )
            .expect("the tree loads");
        let read = document(&Matching::default(), loaded.provenance());
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
