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

use ferrohealth_ui::calendar::{Bounds, Day, Weekday};
use ferrohealth_ui::html::Html;
use ferrohealth_ui::i18n::Locale;
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::table::view::{
    Body, Column, DataTable, Role, Sorting, cell, data_table, title_cell,
};
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::action::ActionGroup;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::date::{DateField, DateWords, date_field};
use ferrohealth_ui::view::form::{Field, INPUT, Inner, SELECT, check_field, help_id, masked_input};
use ferrohealth_ui::view::identifier::{Domain, Masked, identifier_list};
use ferrohealth_ui::view::masked::HoldWords;
use ferrohealth_ui::view::segmented::segmented_links;
use ferrohealth_ui::view::status::badge;
use ferrohealth_ui::view::surface::{card, page_header, stack};
use ferrohealth_ui::view::template::list::{ListCard, list_card, list_empty};
use ferrohealth_ui::view::worklist::{WorkRow, row, worklist};
use ferropix_operator_api::domain::Domains;
use ferropix_operator_api::search::Results;

use crate::app::{SEARCH, SEARCH_DEMOGRAPHICS};
use crate::locale::messages::Msg;
use crate::locale::tr;
use crate::markup::{h, hidden, submit, t};
use crate::model::{Document, Prefill, Segment};
use crate::pages::patient::full_name;
use crate::pages::queue::grade_words;
use crate::pages::{Drawn, Needs, or_dash, server_state};
use crate::visit::Visit;

/// The route the top bar's search panel asks as the steward types.
///
/// It answers the pages, identifier domains and settings the words name,
/// and never a person, since every person search is an audited query
/// (design D39).
pub const SUGGEST: &str = "/search/answer";

/// The submit of a search form.
fn search_submit(visit: &Visit) -> Html {
    submit(
        Variant::Primary.class(),
        &label(Some(Glyph::SEARCH), visit.tr(Msg::SearchSubmit)),
    )
}

/// The form of a search by one identifier, its domain chosen from
/// `domains` when the server sent them.
fn by_identifier(visit: &Visit, domains: Option<&Domains>) -> Html {
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
        Field::new("domain", visit.tr(Msg::Domain)).view(h(format!(
            "<select id=\"domain\" name=\"domain\" class=\"{SELECT}\" disabled><option value=\"\">{}</option></select>",
            t(visit.tr(Msg::DomainsNotKnown))
        )))
    } else {
        let options: Html = choices
            .into_iter()
            .map(|(system, name)| {
                h(format!(
                    "<option value=\"{}\">{}</option>",
                    t(system),
                    t(name)
                ))
            })
            .collect();
        Field::new("domain", visit.tr(Msg::Domain)).view(h(format!(
            "<select id=\"domain\" name=\"domain\" class=\"{SELECT}\" required>{options}</select>"
        )))
    };
    let identifier = Field::new("identifier", visit.tr(Msg::Identifier))
        .trailing(Inner::HoldToShow(
            HoldWords::of(&visit.locale).named(visit.tr(Msg::IdentifierHold)),
        ))
        .view(masked_input(
            "identifier",
            "identifier",
            Some(help_id("identifier")),
        ));
    h(format!(
        "<form method=\"post\" action=\"{SEARCH}\" class=\"fui-form\">{}<div class=\"fui-form-grid\">{domain}<div class=\"fui-span-2\">{identifier}</div><div>{}</div></div><p id=\"{}\" class=\"fui-help\">{}</p></form>",
        hidden("segment", "identifier"),
        search_submit(visit),
        t(help_id("identifier")),
        t(visit.tr(Msg::IdentifierHelp)),
    ))
}

/// A text field of the demographics form, holding `value`.
fn text_field(
    visit: &Visit,
    id: &'static str,
    words: Msg,
    hint: Option<Msg>,
    value: Option<&String>,
) -> Html {
    let field = Field::new(id, visit.tr(words));
    let field = match hint {
        Some(hint) => field.help(visit.tr(hint)),
        None => field,
    };
    let described = hint.map_or_else(String::new, |_| {
        format!(" aria-describedby=\"{}\"", t(help_id(id)))
    });
    field.view(h(format!(
        "<input id=\"{id}\" name=\"{id}\" type=\"text\" class=\"{INPUT}\" autocomplete=\"off\"{described} value=\"{}\">",
        t(value.cloned().unwrap_or_default())
    )))
}

/// The birth date, FerroUI's date field (R23) with its picker (R25), which
/// allows no day after today, holding `value`.
fn birth_date(visit: &Visit, value: Option<&String>) -> Html {
    let today = visit
        .page
        .today
        .and_then(|(year, month, day)| Day::new(year, month, day));
    // NOTE: no specification governs this: our own design; a clock outside the
    // calendar's years leaves the field a plain text field, its format stated.
    let Some(today) = today else {
        return text_field(
            visit,
            "born",
            Msg::BirthDate,
            Some(Msg::BirthDateHint),
            value,
        );
    };
    date_field(DateField {
        id: String::from("born"),
        name: String::from("born"),
        label: visit.tr(Msg::BirthDate),
        value: value.cloned().unwrap_or_default(),
        help: None,
        error: None,
        today,
        bounds: Bounds {
            earliest: None,
            latest: Some(today),
        },
        first_day: visit.preferences().week_start.first_day(Weekday::Monday),
        with_time: false,
        quick: Vec::new(),
        chosen: None,
        words: DateWords::of(&visit.locale, false),
    })
}

/// The form of a search by demographics, holding what `prefill` carried.
fn by_demographics(visit: &Visit, prefill: &Prefill) -> Html {
    let genders: Html = [
        ("", Msg::GenderAny),
        ("female", Msg::GenderFemale),
        ("male", Msg::GenderMale),
        ("other", Msg::GenderOther),
        ("unknown", Msg::GenderUnknown),
    ]
    .into_iter()
    .map(|(value, words)| {
        h(format!(
            "<option value=\"{value}\">{}</option>",
            t(visit.tr(words))
        ))
    })
    .collect();
    let gender = Field::new("gender", visit.tr(Msg::Gender)).view(h(format!(
        "<select id=\"gender\" name=\"gender\" class=\"{SELECT}\">{genders}</select>"
    )));
    let exact = check_field(
        "exact",
        visit.tr(Msg::ExactNames),
        visit.tr(Msg::ExactNamesHelp),
        h(format!(
            "<input id=\"exact\" name=\"exact\" type=\"checkbox\" value=\"on\" aria-describedby=\"{}\">",
            t(help_id("exact"))
        )),
    );
    h(format!(
        "<form method=\"post\" action=\"{SEARCH_DEMOGRAPHICS}\" class=\"fui-form\">{}<div class=\"fui-form-grid\">{}{}{}{gender}{}</div><div class=\"fui-form-foot\">{exact} {}</div></form>",
        hidden("segment", "demographics"),
        text_field(
            visit,
            "family",
            Msg::FamilyName,
            None,
            prefill.family.as_ref()
        ),
        text_field(visit, "given", Msg::GivenNames, None, None),
        birth_date(visit, prefill.born.as_ref()),
        text_field(visit, "postal", Msg::PostalCode, None, None),
        search_submit(visit),
    ))
}

/// The search page, on the segment its route names.
#[must_use]
pub fn search_page(visit: &Visit) -> Drawn {
    let demographics = visit.path() == SEARCH_DEMOGRAPHICS;
    let searched = visit.query().split('&').any(|pair| pair == "searched");
    let (domains, results, prefill) = match &visit.page.document {
        Some(Document::Search {
            domains,
            results,
            prefill,
        }) => (domains.as_ref().ok(), results.as_ref(), prefill.clone()),
        _ => (None, None, Prefill::default()),
    };
    let demographics = demographics || matches!(results, Some((Segment::Demographics, _)));
    let segments = segmented_links(
        visit.tr(Msg::SearchBy),
        vec![
            (String::from(SEARCH), visit.tr(Msg::ByIdentifier)),
            (
                String::from(SEARCH_DEMOGRAPHICS),
                visit.tr(Msg::ByDemographics),
            ),
        ],
        usize::from(demographics),
    );
    let form = if demographics {
        by_demographics(visit, &prefill)
    } else {
        by_identifier(visit, domains)
    };
    let answered = results.map(|(segment, results)| match results {
        Ok(results) => results_table(visit, *segment, results),
        Err(probe) => server_state(
            visit,
            Some(probe),
            Needs::of(Msg::WhatSearch),
            match segment {
                Segment::Identifier => "POST /operator/search/identifier",
                Segment::Demographics => "POST /operator/search/demographics",
            },
        ),
    });
    let outcome = if searched && answered.is_none() {
        let route = if demographics {
            "POST /operator/search/demographics"
        } else {
            "POST /operator/search/identifier"
        };
        server_state(
            visit,
            visit.page.probe.as_ref(),
            Needs::of(Msg::WhatSearch),
            route,
        )
        .then(h(format!(
            "<p class=\"fui-muted\">{}</p>",
            t(visit.tr(Msg::NothingSearched))
        )))
    } else {
        Html::empty()
    };
    let body = page_header(
        &visit.locale,
        visit.tr(Msg::Search),
        Some(visit.tr(Msg::SearchSub)),
        ActionGroup::none(),
    )
    .then(stack(Html::join([
        card(visit.tr(Msg::FindPatient), segments.then(form)),
        answered.unwrap_or_default(),
        outcome,
    ])));
    Drawn::new(visit.tr(Msg::Search), body)
}

/// The results of a search, each row opening its patient; a demographics
/// search adds each person's score and grade.
#[must_use]
pub fn results_table(visit: &Visit, segment: Segment, results: &Results) -> Html {
    let locale = &visit.locale;
    if results.persons.is_empty() {
        return list_card(
            locale,
            ListCard {
                title: visit.tr(Msg::Results),
                count: Some(0_u64.into()),
                order: None,
                tabs: None,
                actions: None,
                body: list_empty(visit.tr(Msg::ResultsEmpty), None),
                pager: None,
            },
        );
    }
    let mut columns = vec![
        Column::text("name", visit.tr(Msg::FullName)).role(Role::Title),
        Column::text("born", visit.tr(Msg::BirthDate)).role(Role::Sub),
        Column::text("gender", visit.tr(Msg::Gender)),
        Column::text("identifiers", visit.tr(Msg::ColIdentifiers)).role(Role::Wide),
    ];
    if segment == Segment::Demographics {
        columns.push(Column::text("grade", visit.tr(Msg::ColGrade)));
    }
    let rows: Html = results
        .persons
        .iter()
        .enumerate()
        .map(|(index, hit)| result_row(visit, segment, &columns, index == 0, hit))
        .collect();
    let caption = visit.tr(Msg::Results);
    let table = data_table(DataTable {
        caption: &caption,
        columns: &columns,
        sorting: Sorting::default(),
        body: Body::Rows(rows),
    });
    list_card(
        locale,
        ListCard {
            title: visit.tr(Msg::Results),
            count: Some(u64::from(results.total).into()),
            order: None,
            tabs: None,
            actions: None,
            body: worklist(table),
            pager: None,
        },
    )
}

/// The work-list row of the person `hit` under `columns`, focused when it
/// is the first; a demographics search adds its grade.
fn result_row(
    visit: &Visit,
    segment: Segment,
    columns: &[Column],
    first: bool,
    hit: &ferropix_operator_api::search::Hit,
) -> Html {
    let locale = &visit.locale;
    let at = |index: usize, value: Html| {
        columns
            .get(index)
            .map_or_else(Html::empty, |column| cell(column, value))
    };
    let leading = hit.leading.clone().unwrap_or_default();
    let name = full_name(&leading).unwrap_or_else(|| visit.tr(Msg::NoName));
    let born = or_dash(locale, leading.birth_date.clone());
    let gender = or_dash(
        locale,
        leading
            .gender
            .as_deref()
            .map(|code| crate::pages::gender_words(locale, code)),
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
    let graded = if segment == Segment::Demographics {
        let words = match (hit.grade, hit.score) {
            (Some(found), Some(score)) => visit.tr_with(
                Msg::GradeScore,
                &[
                    ("grade", grade_words(visit, found).into()),
                    ("score", format!("{score:.2}").into()),
                ],
            ),
            (Some(found), None) => grade_words(visit, found),
            _ => String::from("\u{2013}"),
        };
        at(4, badge(Tone::Info, words))
    } else {
        Html::empty()
    };
    row(WorkRow {
        key: &hit.id,
        focused: first,
        class: None,
        cells: Html::join([
            title_cell(Some(&format!("/patients/{}", hit.id)), t(name), None),
            at(1, born),
            at(2, gender),
            at(3, identifier_list(locale, &chips)),
            graded,
        ]),
    })
}

/// The console's pages the search panel offers, each its address, its
/// words and its glyph.
const SUGGESTED_PAGES: [(&str, Msg, Glyph); 8] = [
    (
        crate::app::DASHBOARD,
        Msg::Dashboard,
        Glyph::LAYOUT_DASHBOARD,
    ),
    (crate::app::QUEUE, Msg::Queue, Glyph::LIST_CHECKS),
    (SEARCH, Msg::Search, Glyph::USER_ROUND_SEARCH),
    (crate::app::SOURCES, Msg::Sources, Glyph::RADIO_TOWER),
    (crate::app::DOMAINS, Msg::Domains, Glyph::TAGS),
    (crate::app::AUDIT, Msg::Audit, Glyph::FILE_CLOCK),
    (
        crate::app::CONFIGURATION,
        Msg::Configuration,
        Glyph::FILE_TEXT,
    ),
    (crate::app::ABOUT, Msg::About, Glyph::INFO),
];

/// What the top bar's search panel offers for `words`.
///
/// That is the console's pages, the identifier domains in `domains` (`None` when the server did not
/// answer, which the panel says) and the settings whose names hold them,
/// never a person (design D39). A patient search runs
/// only on Enter or the panel's last row, as an audited post.
#[must_use]
pub fn suggestions(
    locale: &Locale,
    words: &str,
    domains: Option<&Domains>,
) -> ferrohealth_ui::view::search::SearchAnswer {
    use ferrohealth_ui::view::search::{Hit, HitGroup, Mark, SearchAnswer};
    let wanted = words.trim().to_lowercase();
    let holds = |name: &str| !wanted.is_empty() && name.to_lowercase().contains(&wanted);
    let hit = |href: String, name: String, line: String, glyph: Glyph| Hit {
        href,
        name,
        line,
        mark: Some(Mark::Glyph(glyph)),
        tag: None,
    };
    let pages = SUGGESTED_PAGES
        .into_iter()
        .map(|(href, words, glyph)| (href, tr(locale, words), glyph))
        .filter(|(_, name, _)| holds(name))
        .map(|(href, name, glyph)| {
            hit(
                String::from(href),
                name,
                tr(locale, Msg::SuggestPage),
                glyph,
            )
        })
        .collect::<Vec<_>>();
    let unfinished = domains
        .is_none()
        .then(|| tr(locale, Msg::SuggestDomainsUnread));
    let domains = domains
        .map(|domains| {
            domains
                .domains
                .iter()
                .filter(|domain| holds(&domain.name) || holds(&domain.system))
                .map(|domain| {
                    hit(
                        format!("{}/{}", crate::app::DOMAINS, domain.id),
                        domain.name.clone(),
                        domain.system.clone(),
                        Glyph::TAGS,
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let preferences = crate::app::PREFERENCES;
    let settings = [
        (
            String::from(preferences),
            tr(locale, Msg::SettingAppearance),
        ),
        (
            format!("{preferences}/language"),
            tr(locale, Msg::SettingLanguage),
        ),
        (
            format!("{preferences}/keyboard"),
            tr(locale, Msg::SettingKeyboard),
        ),
    ]
    .into_iter()
    .filter(|(_, name)| holds(name))
    .map(|(href, name)| hit(href, name, tr(locale, Msg::SuggestSetting), Glyph::SETTINGS))
    .collect::<Vec<_>>();
    let groups = [
        (Msg::SuggestPages, pages),
        (Msg::SuggestDomains, domains),
        (Msg::SuggestSettings, settings),
    ]
    .into_iter()
    .filter(|(_, hits)| !hits.is_empty())
    .map(|(kind, hits)| HitGroup {
        kind: tr(locale, kind),
        hits,
    })
    .collect();
    SearchAnswer {
        query: String::from(words.trim()),
        groups,
        none: Some((
            tr(locale, Msg::SuggestNone),
            tr(locale, Msg::SuggestNoneLine),
        )),
        unfinished,
        ..SearchAnswer::default()
    }
}
