// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Search at `/search` and `/search/demographics` (design 04, 18, 18.3):
//! find a patient by one identifier or by demographics.
//!
//! Both forms post, so nothing typed reaches a URL or the browser history
//! (`docs/architecture.md` §12.1, #52); the identifier is masked as it is
//! typed, and the design's hold-to-show eye inside the field waits on
//! FerroUI's R10 (FerroHEALTH/FerroUI#323). The server's search is #90, so until it exists the post sends
//! nothing on and the page says that nothing was searched, under the state
//! the server is in. The domain list is the server's too, so the domain is
//! a disabled choice that says the domains are not known.

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::view::action::ActionGroup;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::form::{Field, INPUT, SELECT, check_field, help_id};
use ferrohealth_ui::view::segmented::segmented_links;
use ferrohealth_ui::view::surface::{card, page_header, stack};
use leptos::prelude::*;

use ferrohealth_ui::calendar::{Bounds, Day, Weekday};
use ferrohealth_ui::island::DatePicker;
use ferrohealth_ui::widget::date::{DateField, DateWords};

use crate::app::{
    SEARCH, SEARCH_DEMOGRAPHICS, current_path, current_query, framed, page_context, preferences,
};
use crate::locale::{Msg, tr};
use crate::pages::{Needs, server_state, title};

/// The submit of a search form.
fn submit() -> AnyView {
    view! {
        <button type="submit" class=Variant::Primary.class()>
            {label(Some(Glyph::SEARCH), tr(Msg::SearchSubmit))}
        </button>
    }
    .into_any()
}

/// The form of a search by one identifier.
fn by_identifier() -> AnyView {
    let domain = Field::new("domain", tr(Msg::Domain)).view(view! {
        <select id="domain" name="domain" class=SELECT disabled>
            <option value="">{tr(Msg::DomainsNotKnown)}</option>
        </select>
    });
    let identifier = Field::new("identifier", tr(Msg::Identifier)).view(view! {
        <input
            id="identifier"
            name="identifier"
            type="password"
            class=INPUT
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            aria-describedby=help_id("identifier")
        />
    });
    view! {
        <form method="post" action=SEARCH class="fui-form">
            <input type="hidden" name="segment" value="identifier" />
            <div class="fui-form-grid">
                {domain}
                <div class="fui-span-2">{identifier}</div>
                <div>{submit()}</div>
            </div>
            <p id=help_id("identifier") class="fui-help">{tr(Msg::IdentifierHelp)}</p>
        </form>
    }
    .into_any()
}

/// A text field of the demographics form.
fn text_field(id: &'static str, words: Msg, hint: Option<Msg>) -> AnyView {
    let field = Field::new(id, tr(words));
    let field = match hint {
        Some(hint) => field.help(tr(hint)),
        None => field,
    };
    let described = hint.map(|_| help_id(id));
    field.view(view! {
        <input id=id name=id type="text" class=INPUT autocomplete="off" aria-describedby=described />
    })
}

/// The birth date, FerroUI's date field (R23) with its picker (R25), which
/// allows no day after today.
fn birth_date() -> AnyView {
    let date = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date();
    let today = u16::try_from(date.year())
        .ok()
        .zip(u8::try_from(date.month()).ok())
        .zip(u8::try_from(date.day()).ok())
        .and_then(|((year, month), day)| Day::new(year, month, day));
    // NOTE: no specification governs this: our own design; a clock outside the
    // calendar's years leaves the field a plain text field, its format stated.
    let Some(today) = today else {
        return text_field("born", Msg::BirthDate, Some(Msg::BirthDateHint));
    };
    let field = DateField {
        id: String::from("born"),
        name: String::from("born"),
        label: tr(Msg::BirthDate),
        value: String::new(),
        help: None,
        error: None,
        today,
        bounds: Bounds {
            earliest: None,
            latest: Some(today),
        },
        first_day: preferences().week_start.first_day(Weekday::Monday),
        with_time: false,
        quick: Vec::new(),
        chosen: None,
        words: DateWords::of_page(false),
    };
    view! { <DatePicker field /> }.into_any()
}

/// The form of a search by demographics.
fn by_demographics() -> AnyView {
    let genders = [
        ("", Msg::GenderAny),
        ("female", Msg::GenderFemale),
        ("male", Msg::GenderMale),
        ("other", Msg::GenderOther),
        ("unknown", Msg::GenderUnknown),
    ]
    .into_iter()
    .map(|(value, words)| view! { <option value=value>{tr(words)}</option> })
    .collect_view();
    let gender = Field::new("gender", tr(Msg::Gender)).view(view! {
        <select id="gender" name="gender" class=SELECT>
            {genders}
        </select>
    });
    let exact = check_field(
        "exact",
        tr(Msg::ExactNames),
        tr(Msg::ExactNamesHelp),
        view! {
            <input
                id="exact"
                name="exact"
                type="checkbox"
                value="on"
                aria-describedby=help_id("exact")
            />
        },
    );
    view! {
        <form method="post" action=SEARCH_DEMOGRAPHICS class="fui-form">
            <input type="hidden" name="segment" value="demographics" />
            <div class="fui-form-grid">
                {text_field("family", Msg::FamilyName, None)}
                {text_field("given", Msg::GivenNames, None)}
                {birth_date()}
                {gender}
                {text_field("postal", Msg::PostalCode, None)}
            </div>
            <div class="fui-form-foot">{exact} {submit()}</div>
        </form>
    }
    .into_any()
}

/// The search page, on the segment its route names.
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn SearchPage() -> impl IntoView {
    let demographics = current_path() == SEARCH_DEMOGRAPHICS;
    let searched = current_query().split('&').any(|pair| pair == "searched");
    let segments = segmented_links(
        tr(Msg::SearchBy),
        vec![
            (String::from(SEARCH), tr(Msg::ByIdentifier)),
            (String::from(SEARCH_DEMOGRAPHICS), tr(Msg::ByDemographics)),
        ],
        usize::from(demographics),
    );
    let form = if demographics {
        by_demographics()
    } else {
        by_identifier()
    };
    let outcome = searched.then(|| {
        let probe = page_context().probe;
        let needs = Needs {
            what: Msg::WhatSearch,
            away: false,
        };
        let route = if demographics {
            "POST /operator/search/demographics"
        } else {
            "POST /operator/search/identifier"
        };
        view! {
            {server_state(probe.as_ref(), needs, route)}
            <p class="fui-muted">{tr(Msg::NothingSearched)}</p>
        }
    });
    framed(
        view! {
            {title(tr(Msg::Search))}
            {page_header(tr(Msg::Search), Some(tr(Msg::SearchSub)), ActionGroup::none())}
            {stack(view! {
                {card(tr(Msg::FindPatient), view! { {segments} {form} })}
                {outcome}
            })}
        }
        .into_any(),
    )
}
