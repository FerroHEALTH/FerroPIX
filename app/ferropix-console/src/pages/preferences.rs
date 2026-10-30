// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Your preferences at `/preferences` and its tabs (design 21).
//!
//! FerroUI's three tabs, Appearance, Language and region and Keyboard,
//! with the console's own two rows, the reveal duration and hide on leave,
//! inside Appearance (FerroUI COMPONENTS.md T3, family rule 9).
//!
//! The page is server HTML; each preference posts to the preference route,
//! which keeps it in its cookie and returns with `?saved=` naming it, so
//! the row says Saved.

use ferrohealth_ui::calendar::Weekday;
use ferrohealth_ui::html::Html;
use ferrohealth_ui::i18n::{Arg, Locale};
use ferrohealth_ui::view::preferences::{
    Post, PreferenceRow, Section, SectionRoutes, SelectPreference, SwitchPreference,
    preference_row, preferences_page as library_page, select_preference, switch_preference,
};

use crate::app::PREFERENCES;
use crate::locale::choices;
use crate::locale::messages::Msg;
use crate::pages::Drawn;
use crate::pages::not_found::not_found;
use crate::preferences::{
    HIDE, PREFERENCE_ROUTE, RESET_ROUTE, REVEAL, REVEAL_CHOICES, REVEAL_DEFAULT,
};
use crate::visit::Visit;

/// The console's own preference `name` the request keeps.
fn own(visit: &Visit, name: &str) -> Option<String> {
    crate::preferences::own_in(&visit.page.preferences, &visit.page.prefix, name)
}

/// The console's own rows in Appearance: how long a revealed identifier
/// stays, and whether leaving the page hides it.
fn reveal_rows(visit: &Visit, post: &Post, saved: Option<&str>) -> Vec<Html> {
    let locale = &visit.locale;
    let reveal_saved = saved == Some(REVEAL);
    let hide_saved = saved == Some(HIDE);
    let reveal = select_preference(
        locale,
        SelectPreference {
            name: String::from(REVEAL),
            choices: REVEAL_CHOICES
                .into_iter()
                .map(|seconds| {
                    let count = seconds.parse::<u32>().unwrap_or_default();
                    (
                        String::from(seconds),
                        visit
                            .tr_with(Msg::RevealSeconds, &[("count", Arg::from(count))])
                            .into(),
                    )
                })
                .collect(),
            current: own(visit, REVEAL).unwrap_or_else(|| String::from(REVEAL_DEFAULT)),
            post: post.clone(),
            saved: reveal_saved,
        },
    );
    let hide = switch_preference(
        locale,
        SwitchPreference {
            name: String::from(HIDE),
            label: visit.tr(Msg::HideOnLeaveSwitch),
            on: own(visit, HIDE).as_deref() != Some("off"),
            post: post.clone(),
            saved: hide_saved,
        },
    );
    vec![
        preference_row(
            locale,
            PreferenceRow {
                label: visit.tr(Msg::RevealFor).into(),
                description: visit.tr(Msg::RevealForDesc).into(),
                control_id: Some(String::from(REVEAL)),
                control: reveal,
                saved: reveal_saved,
            },
        ),
        preference_row(
            locale,
            PreferenceRow {
                label: visit.tr(Msg::HideOnLeave).into(),
                description: visit.tr(Msg::HideOnLeaveDesc).into(),
                control_id: Some(String::from(HIDE)),
                control: hide,
                saved: hide_saved,
            },
        ),
    ]
}

/// Your preferences, on the tab the route names.
#[must_use]
pub fn preferences_page(visit: &Visit) -> Drawn {
    let section = visit
        .param("section")
        .map_or(Some(Section::Appearance), |name| Section::parse(&name));
    let Some(section) = section else {
        return not_found(visit);
    };
    let routes = SectionRoutes {
        appearance: String::from(PREFERENCES),
        language: format!("{PREFERENCES}/language"),
        keyboard: format!("{PREFERENCES}/keyboard"),
    };
    let saved = visit
        .query_word("saved")
        .filter(|name| name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .map(str::to_owned);
    let post = Post {
        action: String::from(PREFERENCE_ROUTE),
        back: routes.of(section).to_owned(),
    };
    let own = match section {
        Section::Appearance => reveal_rows(visit, &post, saved.as_deref()),
        _ => Vec::new(),
    };
    let tag = &visit.page.browser;
    let browser = choices()
        .into_iter()
        .find(|locale| locale.tag() == *tag)
        .unwrap_or_else(Locale::english);
    let page = library_page(
        &visit.locale,
        ferrohealth_ui::view::preferences::PreferencesPage {
            section,
            routes,
            reading: false,
            preferences: visit.preferences(),
            action: String::from(PREFERENCE_ROUTE),
            saved,
            accent: visit.tr(Msg::AccentPlum),
            languages: choices(),
            browser,
            week_from_language: Weekday::Monday,
            shortcuts: None,
            reset: String::from(RESET_ROUTE),
            own,
        },
    );
    Drawn::new(visit.tr(Msg::Preferences), page)
}
