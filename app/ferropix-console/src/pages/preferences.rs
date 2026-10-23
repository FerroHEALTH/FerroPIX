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
use ferrohealth_ui::i18n::Arg;
use ferrohealth_ui::view::preferences::{
    Post, PreferenceRow, Section, SectionRoutes, SelectPreference, SwitchPreference,
    preference_row, preferences_page, select_preference, switch_preference,
};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::app::{PREFERENCES, current_query, framed, page_context, preferences};
use crate::locale::{Msg, choices, negotiated, tr, tr_with};
use crate::pages::not_found::NotFound;
use crate::pages::title;
use crate::preferences::{
    HIDE, PREFERENCE_ROUTE, RESET_ROUTE, REVEAL, REVEAL_CHOICES, REVEAL_DEFAULT,
};
use crate::server::ConsoleState;

/// The console's own preference `name` the request keeps.
fn own(name: &str) -> Option<String> {
    let prefix = page_context().prefix;
    use_context::<http::request::Parts>()
        .and_then(|parts| crate::preferences::own(&parts.headers, &prefix, name))
}

/// The console's own rows in Appearance: how long a revealed identifier
/// stays, and whether leaving the page hides it.
fn reveal_rows(post: &Post, saved: Option<&str>) -> Vec<AnyView> {
    let reveal_saved = saved == Some(REVEAL);
    let hide_saved = saved == Some(HIDE);
    let reveal = select_preference(SelectPreference {
        name: String::from(REVEAL),
        choices: REVEAL_CHOICES
            .into_iter()
            .map(|seconds| {
                let count = seconds.parse::<u32>().unwrap_or_default();
                (
                    String::from(seconds),
                    tr_with(Msg::RevealSeconds, &[("count", Arg::from(count))]).into(),
                )
            })
            .collect(),
        current: own(REVEAL).unwrap_or_else(|| String::from(REVEAL_DEFAULT)),
        post: post.clone(),
        saved: reveal_saved,
    });
    let hide = switch_preference(SwitchPreference {
        name: String::from(HIDE),
        label: tr(Msg::HideOnLeaveSwitch),
        on: own(HIDE).as_deref() != Some("off"),
        post: post.clone(),
        saved: hide_saved,
    });
    vec![
        preference_row(PreferenceRow {
            label: tr(Msg::RevealFor).into(),
            description: tr(Msg::RevealForDesc).into(),
            control_id: Some(String::from(REVEAL)),
            control: reveal,
            saved: reveal_saved,
        }),
        preference_row(PreferenceRow {
            label: tr(Msg::HideOnLeave).into(),
            description: tr(Msg::HideOnLeaveDesc).into(),
            control_id: Some(String::from(HIDE)),
            control: hide,
            saved: hide_saved,
        }),
    ]
}

/// Your preferences, on the tab the route names.
#[component]
pub fn PreferencesPage() -> impl IntoView {
    let section = use_params_map().with_untracked(|params| {
        params
            .get_str("section")
            .map_or(Some(Section::Appearance), Section::parse)
    });
    let Some(section) = section else {
        return view! { <NotFound /> }.into_any();
    };
    let routes = SectionRoutes {
        appearance: String::from(PREFERENCES),
        language: format!("{PREFERENCES}/language"),
        keyboard: format!("{PREFERENCES}/keyboard"),
    };
    let saved = current_query()
        .split('&')
        .find_map(|pair| pair.strip_prefix("saved="))
        .filter(|name| name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .map(str::to_owned);
    let post = Post {
        action: String::from(PREFERENCE_ROUTE),
        back: routes.of(section).to_owned(),
    };
    let own = match section {
        Section::Appearance => reveal_rows(&post, saved.as_deref()),
        _ => Vec::new(),
    };
    let configured = use_context::<ConsoleState>()
        .map(|state| state.settings().locale.clone())
        .unwrap_or_default();
    let browser = use_context::<http::request::Parts>().map_or(configured.clone(), |parts| {
        negotiated(&parts.headers, configured)
    });
    let page = preferences_page(ferrohealth_ui::view::preferences::PreferencesPage {
        section,
        routes,
        reading: false,
        preferences: preferences(),
        action: String::from(PREFERENCE_ROUTE),
        saved,
        accent: tr(Msg::AccentPlum),
        languages: choices(),
        browser,
        week_from_language: Weekday::Monday,
        shortcuts: None,
        reset: String::from(RESET_ROUTE),
        own,
    });
    framed(
        view! {
            {title(tr(Msg::Preferences))}
            {page}
        }
        .into_any(),
    )
}
