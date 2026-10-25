// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Search at `/search` and `/search/demographics` (design 04, 18, 18.3):
//! find a patient by one identifier or by demographics.
//!
//! Both forms post, so nothing typed reaches a URL or the browser history
//! (`docs/architecture.md` §12.1, #52); the identifier is masked as it is
//! typed, with FerroUI's hold-to-show eye inside the field (R10). The
//! server's search is #90, so until it exists the post sends
//! nothing on and the page says that nothing was searched, under the state
//! the server is in. The domain list is the server's too, so the domain is
//! a disabled choice that says the domains are not known.

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::view::action::ActionGroup;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::form::{Field, INPUT, Inner, SELECT, check_field, help_id, masked_input};
use ferrohealth_ui::view::segmented::segmented_links;
use ferrohealth_ui::view::surface::{card, page_header, stack};
use ferrohealth_ui::widget::masked::HoldWords;
use leptos::prelude::*;

use ferrohealth_ui::calendar::{Bounds, Day, Weekday};
use ferrohealth_ui::island::DatePicker;
use ferrohealth_ui::widget::date::{DateField, DateWords};

use crate::app::{
    SEARCH, SEARCH_DEMOGRAPHICS, current_path, current_query, framed, page_context, preferences,
};
use crate::document::{Document, Prefill, Segment};
use crate::locale::{Msg, tr, tr_with};
use crate::pages::patient::full_name;
use crate::pages::{Needs, server_state, title};
use ferrohealth_ui::island::WorkList;
use ferrohealth_ui::table::view::{
    Body, Column, DataTable, Role, Sorting, cell, data_table, title_cell,
};
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::status::badge;
use ferrohealth_ui::view::surface::or_none;
use ferrohealth_ui::view::template::list::{ListCard, list_card, list_empty};
use ferrohealth_ui::widget::identifier::{Domain, Masked, identifier_list};
use ferrohealth_ui::widget::worklist::{WorkRow, row};
use ferropix_operator_api::domain::Domains;
use ferropix_operator_api::patient::Grade;
use ferropix_operator_api::search::Results;

/// The submit of a search form.
fn submit() -> AnyView {
    view! {
        <button type="submit" class=Variant::Primary.class()>
            {label(Some(Glyph::SEARCH), tr(Msg::SearchSubmit))}
        </button>
    }
    .into_any()
}

/// The form of a search by one identifier, its domain chosen from
/// `domains` when the server sent them.
fn by_identifier(domains: Option<&Domains>) -> AnyView {
    let choices: Vec<(String, String)> = domains
        .map(|domains| {
            domains
                .domains
                .iter()
                .filter(|domain| domain.retired_at.is_none())
                .map(|domain| (domain.system.clone(), domain.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let domain = if choices.is_empty() {
        Field::new("domain", tr(Msg::Domain)).view(view! {
            <select id="domain" name="domain" class=SELECT disabled>
                <option value="">{tr(Msg::DomainsNotKnown)}</option>
            </select>
        })
    } else {
        let options = choices
            .into_iter()
            .map(|(system, name)| view! { <option value=system>{name}</option> })
            .collect_view();
        Field::new("domain", tr(Msg::Domain)).view(view! {
            <select id="domain" name="domain" class=SELECT required>
                {options}
            </select>
        })
    };
    let identifier = Field::new("identifier", tr(Msg::Identifier))
        .trailing(Inner::HoldToShow(
            HoldWords::of_page().named(tr(Msg::IdentifierHold)),
        ))
        .view(masked_input(
            "identifier",
            "identifier",
            Some(help_id("identifier")),
        ));
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

/// A text field of the demographics form, holding `value`.
fn text_field(id: &'static str, words: Msg, hint: Option<Msg>, value: Option<&String>) -> AnyView {
    let field = Field::new(id, tr(words));
    let field = match hint {
        Some(hint) => field.help(tr(hint)),
        None => field,
    };
    let described = hint.map(|_| help_id(id));
    field.view(view! {
        <input id=id name=id type="text" class=INPUT autocomplete="off" aria-describedby=described value=value.cloned().unwrap_or_default() />
    })
}

/// The birth date, FerroUI's date field (R23) with its picker (R25), which
/// allows no day after today, holding `value`.
fn birth_date(value: Option<&String>) -> AnyView {
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
        return text_field("born", Msg::BirthDate, Some(Msg::BirthDateHint), value);
    };
    let field = DateField {
        id: String::from("born"),
        name: String::from("born"),
        label: tr(Msg::BirthDate),
        value: value.cloned().unwrap_or_default(),
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

/// The form of a search by demographics, holding what `prefill` carried.
fn by_demographics(prefill: &Prefill) -> AnyView {
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
                {text_field("family", Msg::FamilyName, None, prefill.family.as_ref())}
                {text_field("given", Msg::GivenNames, None, None)}
                {birth_date(prefill.born.as_ref())}
                {gender}
                {text_field("postal", Msg::PostalCode, None, None)}
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
    let document = page_context().document;
    let (domains, results, prefill) = match document {
        Some(Document::Search {
            domains,
            results,
            prefill,
        }) => (domains.ok(), results, prefill),
        _ => (None, None, Prefill::default()),
    };
    let demographics = demographics || matches!(results, Some((Segment::Demographics, _)));
    let segments = segmented_links(
        tr(Msg::SearchBy),
        vec![
            (String::from(SEARCH), tr(Msg::ByIdentifier)),
            (String::from(SEARCH_DEMOGRAPHICS), tr(Msg::ByDemographics)),
        ],
        usize::from(demographics),
    );
    let form = if demographics {
        by_demographics(&prefill)
    } else {
        by_identifier(domains.as_ref())
    };
    let answered = results.map(|(segment, results)| match results {
        Ok(results) => results_table(segment, &results),
        Err(probe) => server_state(
            Some(&probe),
            Needs {
                what: Msg::WhatSearch,
                away: false,
            },
            match segment {
                Segment::Identifier => "POST /operator/search/identifier",
                Segment::Demographics => "POST /operator/search/demographics",
            },
        ),
    });
    let outcome = (searched && answered.is_none()).then(|| {
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
                {answered}
                {outcome}
            })}
        }
        .into_any(),
    )
}

/// The words of a grade.
fn grade(grade: Grade) -> String {
    tr(match grade {
        Grade::Certain => Msg::GradeCertain,
        Grade::Probable => Msg::GradeProbable,
        Grade::Possible => Msg::GradePossible,
        Grade::CertainlyNot => Msg::GradeCertainlyNot,
    })
}

/// The results of a search, each row opening its patient; a demographics
/// search adds each person's score and grade.
#[must_use]
pub fn results_table(segment: Segment, results: &Results) -> AnyView {
    if results.persons.is_empty() {
        return list_card(ListCard {
            title: tr(Msg::Results),
            count: Some(0_u64.into()),
            order: None,
            tabs: None,
            actions: None,
            body: list_empty(tr(Msg::ResultsEmpty), None),
            pager: None,
        });
    }
    let mut columns = vec![
        Column::text("name", tr(Msg::FullName)).role(Role::Title),
        Column::text("born", tr(Msg::BirthDate)).role(Role::Sub),
        Column::text("gender", tr(Msg::Gender)),
        Column::text("identifiers", tr(Msg::ColIdentifiers)).role(Role::Wide),
    ];
    if segment == Segment::Demographics {
        columns.push(Column::text("grade", tr(Msg::ColGrade)));
    }
    let rows = results
        .persons
        .iter()
        .enumerate()
        .map(|(index, hit)| {
            let leading = hit.leading.clone().unwrap_or_default();
            let name = full_name(&leading).unwrap_or_else(|| tr(Msg::NoName));
            let born = leading
                .birth_date
                .clone()
                .map_or_else(|| or_none(String::new()), IntoAny::into_any);
            let gender = leading.gender.as_deref().map_or_else(
                || or_none(String::new()),
                |code| crate::pages::gender_words(code).into_any(),
            );
            let chips: Vec<Masked> = hit
                .identifiers
                .iter()
                .map(|identifier| Masked {
                    domain: Domain::new(identifier.domain.name.clone())
                        .system(identifier.domain.system.clone()),
                    tail: identifier
                        .masked
                        .0
                        .trim_start_matches('\u{2022}')
                        .trim()
                        .to_owned(),
                })
                .collect();
            let graded = (segment == Segment::Demographics).then(|| {
                let words = match (hit.grade, hit.score) {
                    (Some(found), Some(score)) => tr_with(
                        Msg::GradeScore,
                        &[
                            ("grade", grade(found).into()),
                            ("score", format!("{score:.2}").into()),
                        ],
                    ),
                    (Some(found), None) => grade(found),
                    _ => String::from("\u{2013}"),
                };
                columns
                    .get(4)
                    .map(|column| cell(column, badge(Tone::Info, words)))
            });
            row(WorkRow {
                key: &hit.id,
                focused: index == 0,
                class: None,
                cells: view! {
                    {title_cell(Some(&format!("/patients/{}", hit.id)), name, None)}
                    {columns.get(1).map(|column| cell(column, born))}
                    {columns.get(2).map(|column| cell(column, gender))}
                    {columns.get(3).map(|column| cell(column, identifier_list(&chips)))}
                    {graded}
                }
                .into_any(),
            })
        })
        .collect_view()
        .into_any();
    list_card(ListCard {
        title: tr(Msg::Results),
        count: Some(u64::from(results.total).into()),
        order: None,
        tabs: None,
        actions: None,
        body: {
            let table = data_table(DataTable {
                caption: &tr(Msg::Results),
                columns: &columns,
                sorting: Sorting::default(),
                body: Body::Rows(rows),
            })
            .into_any();
            view! { <WorkList>{table}</WorkList> }.into_any()
        },
        pager: None,
    })
}
