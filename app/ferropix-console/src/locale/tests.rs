// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The catalog's tests: every message present, and the pseudo-locale.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use ferrohealth_ui::i18n::Locale;
use ferrohealth_ui::theme::{Language, Preferences};
use fluent_templates::fluent_bundle::FluentValue;

use super::{Msg, PSEUDO_TAG, catalog, choices, chosen, negotiated, pseudo, stretch, tr_in};

fluent_templates::static_loader! {
    static THROWAWAY = {
        locales: "./tests/locales",
        fallback_language: "qaa",
        customise: |bundle| bundle.set_use_isolating(false),
    };
}

/// Every placeable the catalog at `path` names, each set to a number, so
/// a message that selects a form by one formats whole.
fn placeables(path: &Path) -> HashMap<Cow<'static, str>, FluentValue<'static>> {
    let source = std::fs::read_to_string(path).expect("the catalog");
    source
        .split("{ $")
        .skip(1)
        .filter_map(|rest| rest.split([' ', '}']).next())
        .map(|name| (Cow::Owned(name.to_owned()), FluentValue::from(3)))
        .collect()
}

#[test]
fn every_catalog_directory_is_a_language_holding_every_message_and_no_other() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    let mut found: Vec<String> = std::fs::read_dir(&root)
        .expect("the locales directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    found.sort();
    let mut shipped: Vec<String> = catalog().locales().map(ToString::to_string).collect();
    shipped.sort();
    assert_eq!(found, shipped, "a catalog directory is a language");
    assert!(
        found.contains(&String::from("en")),
        "English is the reference"
    );
    for tag in found {
        let language = tag.parse().expect("a language");
        let path = root.join(&tag).join("ferropix-console.ftl");
        let args = placeables(&path);
        for msg in Msg::ALL {
            let words = catalog().try_lookup_with_args(&language, msg.id(), &args);
            assert!(
                words.is_some_and(|words| !words.trim().is_empty()),
                "{tag} holds {}",
                msg.id()
            );
        }
        let source = std::fs::read_to_string(&path).expect("the catalog");
        let mut held: Vec<&str> = source
            .lines()
            .filter_map(|line| line.split_once(" ="))
            .map(|(id, _)| id)
            .filter(|id| id.starts_with(|c: char| c.is_ascii_lowercase()) && !id.contains(' '))
            .filter(|id| !id.starts_with("number-"))
            .collect();
        held.sort_unstable();
        let mut declared: Vec<&str> = Msg::ALL.iter().map(|msg| msg.id()).collect();
        declared.sort_unstable();
        assert_eq!(
            declared, held,
            "{tag} holds every message once and no other"
        );
        assert!(
            source.contains("number-group ="),
            "{tag} names its digit group mark"
        );
        assert!(
            source.contains("number-decimal ="),
            "{tag} names its decimal mark"
        );
    }
}

#[test]
fn a_throwaway_catalog_changes_the_words_with_no_code_change() {
    let english = Locale::english();
    assert_eq!(
        "Page not found",
        tr_in(catalog(), &english, Msg::NotFoundTitle)
    );
    let throwaway = Locale::parse("qaa").expect("a private-use language");
    assert_eq!(
        "Qage qot qound",
        tr_in(&*THROWAWAY, &throwaway, Msg::NotFoundTitle)
    );
    assert_eq!(
        "Back to the dashboard",
        tr_in(catalog(), &throwaway, Msg::NotFoundBack),
        "the shipped catalog falls back to English"
    );
}

#[test]
fn the_pseudo_locale_accents_and_lengthens_every_string() {
    assert_eq!("[Págé ~~~]", stretch("Page"));
    assert_eq!(
        "[Báçk tö thé dáshböárd ~~~~~ ~~~~]",
        stretch("Back to the dashboard"),
        "the padding breaks into words of at most five"
    );
    let words = tr_in(catalog(), &pseudo(), Msg::NotFoundTitle);
    assert_eq!(stretch("Page not found"), words, "a console word stretches");
    assert_eq!(
        Some(stretch("Series")),
        catalog().try_lookup(&PSEUDO_TAG.parse().expect("a language"), "chart-series"),
        "a library word stretches too, so the library's own words follow the page"
    );
    assert_eq!(
        PSEUDO_TAG,
        pseudo().tag(),
        "<html lang> names the pseudo-locale"
    );
}

#[test]
fn only_a_language_cookie_naming_the_pseudo_locale_chooses_it() {
    assert!(
        choices().iter().all(|locale| locale.tag() != PSEUDO_TAG),
        "the language preference never offers the pseudo-locale"
    );
    let mut asking = http::HeaderMap::new();
    asking.insert(
        http::header::ACCEPT_LANGUAGE,
        http::HeaderValue::from_static("qps"),
    );
    assert_eq!(
        "en",
        negotiated(&asking, Locale::english()).tag(),
        "a browser asking for it is drawn in the configured language"
    );
    let cookie = Preferences {
        language: Language::Chosen(String::from(PSEUDO_TAG)),
        ..Preferences::default()
    };
    assert_eq!(
        PSEUDO_TAG,
        chosen(&cookie, &http::HeaderMap::new(), Locale::english()).tag()
    );
}
