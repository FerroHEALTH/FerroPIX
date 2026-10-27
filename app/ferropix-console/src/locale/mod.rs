// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's own words, one Fluent catalog per language under `locales/`.
//!
//! Each catalog is embedded at build time and looked up in the locale the
//! request chose, so the console's words and the library's follow one
//! choice (`.claude/rules/leptos-ui.md`, FerroUI §13).
//!
//! English is the one catalog that ships and the reference every lookup
//! falls back to; a language joins as one directory with no code change.
//! No specification governs the catalog layout: our own design; the message
//! syntax is Project Fluent's.
//!
//! The pseudo-locale, tagged [`PSEUDO_TAG`], draws every console string and
//! every library string accented and two fifths longer, so the browser
//! journeys see a word that skipped the catalog and a layout that does not
//! leave room for a longer translation. It ships no catalog file and the
//! language preference never offers it: only a language cookie naming it
//! chooses it (FerroUI §13, after the gallery's pseudo-locale).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;

use ferrohealth_ui::i18n::{Arg, Locale, library_catalog, locale};
use ferrohealth_ui::theme::Preferences;
use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::{LanguageIdentifier, Loader};

use messages::Msg;

pub mod messages;

/// The tag of the pseudo-locale, a private-use language subtag (BCP 47,
/// `qaa` to `qtz`), so no catalog the console or the library ships serves
/// it.
pub const PSEUDO_TAG: &str = "qps";

/// The pseudo-locale's language.
static PSEUDO_LANGUAGE: LazyLock<LanguageIdentifier> =
    LazyLock::new(|| fluent_templates::langid!("qps"));

fluent_templates::static_loader! {
    static SHIPPED = {
        locales: "./locales",
        fallback_language: "en",
        // NOTE: Fluent's guide, "Bidirectionality": the isolation marks around a
        // placeable are for mixed-direction text; the English catalog draws none.
        customise: |bundle| {
            bundle.set_use_isolating(false);
            bundle.set_formatter(Some(ferrohealth_ui::i18n::number::format_number));
        },
    };
}

/// The console's catalog: its own words in every language it ships, and in
/// the pseudo-locale the library's English and its own, stretched.
#[derive(Debug, Clone, Copy)]
pub struct Catalog;

/// The one catalog the console looks its words up in and hands the library.
static CATALOG: Catalog = Catalog;

/// The console's catalog, every language it holds.
#[must_use]
pub fn catalog() -> &'static (dyn Loader + Send + Sync) {
    &CATALOG
}

impl Loader for Catalog {
    fn lookup_complete(
        &self,
        lang: &LanguageIdentifier,
        text_id: &str,
        args: Option<&HashMap<Cow<'static, str>, FluentValue>>,
    ) -> String {
        self.try_lookup_complete(lang, text_id, args)
            .unwrap_or_else(|| text_id.to_owned())
    }

    fn try_lookup_complete(
        &self,
        lang: &LanguageIdentifier,
        text_id: &str,
        args: Option<&HashMap<Cow<'static, str>, FluentValue>>,
    ) -> Option<String> {
        if *lang != *PSEUDO_LANGUAGE {
            return SHIPPED.try_lookup_complete(lang, text_id, args);
        }
        let english = fluent_templates::langid!("en");
        SHIPPED
            .try_lookup_complete(&english, text_id, args)
            .or_else(|| library_catalog().try_lookup_complete(&english, text_id, args))
            .map(|english| stretch(&english))
    }

    // NOTE: no specification governs this: our own design; the pseudo-locale is
    // left out, so neither the language preference nor `Accept-Language` reaches it.
    fn locales(&self) -> Box<dyn Iterator<Item = &LanguageIdentifier> + '_> {
        SHIPPED.locales()
    }
}

/// `english` accented letter by letter and padded by two fifths of its
/// length (at least three), in brackets, so a cut end shows.
///
/// The padding is tildes in words of at most five, so a long message breaks
/// across lines as a translation would, where one run of tildes could not.
#[must_use]
pub fn stretch(english: &str) -> String {
    let accented: String = english.chars().map(accent).collect();
    let length = english.chars().count();
    let pad = length.saturating_mul(2).div_ceil(5).max(3);
    let words: Vec<String> = (0..pad.div_ceil(PAD_WORD))
        .map(|word| "~".repeat(PAD_WORD.min(pad.saturating_sub(word.saturating_mul(PAD_WORD)))))
        .collect();
    format!("[{accented} {}]", words.join(" "))
}

/// The longest word of tildes the pseudo-locale pads a message with.
const PAD_WORD: usize = 5;

/// The accented look-alike of `letter`, the letter itself when it has none.
const fn accent(letter: char) -> char {
    match letter {
        'a' => 'á',
        'c' => 'ç',
        'e' => 'é',
        'i' => 'í',
        'n' => 'ñ',
        'o' => 'ö',
        'u' => 'ü',
        'y' => 'ý',
        'A' => 'Å',
        'C' => 'Ç',
        'E' => 'É',
        'I' => 'Î',
        'N' => 'Ñ',
        'O' => 'Ö',
        'U' => 'Ü',
        'Y' => 'Ý',
        other => other,
    }
}

/// The pseudo-locale, with the console's catalog consulted for every
/// message, the library's included.
///
/// # Panics
///
/// Never: [`PSEUDO_TAG`] is a constant private-use language subtag, which
/// every parse accepts.
#[must_use]
pub fn pseudo() -> Locale {
    #[expect(
        clippy::expect_used,
        reason = "PSEUDO_TAG is a constant private-use language subtag, which every parse accepts"
    )]
    let language = Locale::parse(PSEUDO_TAG).expect("the pseudo-locale's tag should parse");
    language.with_catalog(catalog())
}

/// The console's message `msg` in the page's locale.
#[must_use]
pub fn tr(msg: Msg) -> String {
    tr_in(catalog(), &locale(), msg)
}

/// The console's message `msg` in the page's locale, with the placeables
/// `args` filled in.
#[must_use]
pub fn tr_with(msg: Msg, args: &[(&'static str, Arg)]) -> String {
    locale().lookup_with(catalog(), msg.id(), args)
}

/// The message `msg` of `catalog` in `locale`: for a handler, where no page
/// tree is in reach.
#[must_use]
pub fn tr_in(catalog: &dyn Loader, locale: &Locale, msg: Msg) -> String {
    locale.lookup(catalog, msg.id())
}

/// The locales the language preference offers: every catalog the console
/// ships, English first.
#[must_use]
pub fn choices() -> Vec<Locale> {
    let mut tags: Vec<String> = catalog().locales().map(ToString::to_string).collect();
    tags.sort();
    // NOTE: no specification governs this: our own design; a directory under
    // `locales/` that is no language tag is a build defect the catalog test reports.
    let mut found: Vec<Locale> = tags
        .iter()
        .filter(|tag| tag.as_str() != "en")
        .filter_map(|tag| Locale::parse(tag).ok())
        .collect();
    found.insert(0, Locale::english());
    found
}

/// The locale of a request with `headers`: the best language of its
/// `Accept-Language` the catalog holds, else `configured`.
#[must_use]
pub fn negotiated(headers: &http::HeaderMap, configured: Locale) -> Locale {
    // NOTE: RFC 9110 §12.5.4: a header that is no visible ASCII names no
    // language, so the request falls to the configured one.
    let header = headers
        .get(http::header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    Locale::negotiate_with(header, catalog(), configured)
}

/// The locale a page is drawn in: the language the steward chose in Your
/// preferences when the console ships it or it is the pseudo-locale, else
/// the request's negotiated one.
#[must_use]
pub fn chosen(preferences: &Preferences, headers: &http::HeaderMap, configured: Locale) -> Locale {
    match preferences.language.tag() {
        Some(PSEUDO_TAG) => pseudo(),
        tag => tag
            .and_then(|tag| choices().into_iter().find(|locale| locale.tag() == tag))
            .unwrap_or_else(|| negotiated(headers, configured)),
    }
}

#[cfg(test)]
mod tests;
