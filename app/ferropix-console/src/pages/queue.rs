// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The review queue, drawn from `GET /operator/candidates`
//! (`docs/architecture.md` §5.3 and §12.3).
//!
//! The queue is FerroUI's work-list in its list card: one row per
//! candidate, the two leading names, their birth dates and domains, the
//! score bar with its figure, the grade and the age, walked with `j` and
//! `k` and opened with Enter. Each row opens its pair in a side panel (`o`
//! or `d`), which holds Open to decide and the deferral. The filters and
//! the paging are posted forms, so the queue's state stays out of the
//! address, and a row names people, never a business identifier. No
//! specification governs the queue: our own design.

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::island::{SidePanel, WorkList};
use ferrohealth_ui::table::view::{
    Body, Column, DataTable, Role, Sorting, cell, data_table, title_cell,
};
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::form::{Field, INPUT, SELECT, check_field, help_id};
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::score::score_bar;
use ferrohealth_ui::view::status::pill;
use ferrohealth_ui::view::surface::{card, definitions, or_none, stack};
use ferrohealth_ui::view::template::list::{ListCard, list_card, list_empty};
use ferrohealth_ui::view::time::timestamp;
use ferrohealth_ui::widget::side_panel::{Opener, SidePanelWords};
use ferrohealth_ui::widget::worklist::{WorkRow, row, worklist_foot};
use ferropix_operator_api::candidate::{
    CandidateQuery, CandidateRow, Candidates, PAGE, Side, Status,
};
use ferropix_operator_api::patient::Grade;
use leptos::prelude::*;

use crate::app::{DASHBOARD, QUEUE};
use crate::locale::messages::Msg;
use crate::locale::{tr, tr_with};
use crate::model::Read;
use crate::pages::{Needs, server_state};

/// The hidden field that names which of the queue's forms was posted.
pub const FORM: &str = "form";

/// The value of [`FORM`] for the filters and the paging.
pub const FILTER: &str = "filter";

/// The value of [`FORM`] for a deferral from a row's side panel.
pub const DEFER: &str = "defer";

/// The words of a grade.
#[must_use]
pub fn grade_words(grade: Grade) -> String {
    tr(match grade {
        Grade::Certain => Msg::GradeCertain,
        Grade::Probable => Msg::GradeProbable,
        Grade::Possible => Msg::GradePossible,
        Grade::CertainlyNot => Msg::GradeCertainlyNot,
    })
}

/// The tone of a grade, the score breakdown's band tones.
#[must_use]
pub const fn grade_tone(grade: Grade) -> Tone {
    match grade {
        Grade::Certain => Tone::Ok,
        Grade::Probable => Tone::Info,
        Grade::Possible => Tone::Warn,
        Grade::CertainlyNot => Tone::Neutral,
    }
}

/// The key of a grade on the wire.
#[must_use]
pub const fn grade_key(grade: Grade) -> &'static str {
    match grade {
        Grade::Certain => "certain",
        Grade::Probable => "probable",
        Grade::Possible => "possible",
        Grade::CertainlyNot => "certainly-not",
    }
}

/// The grade a key names, `None` for any other text.
#[must_use]
pub fn grade_of(key: &str) -> Option<Grade> {
    [
        Grade::Certain,
        Grade::Probable,
        Grade::Possible,
        Grade::CertainlyNot,
    ]
    .into_iter()
    .find(|grade| grade_key(*grade) == key)
}

/// The queue: the outcome of a deferral just posted, the filters, and the
/// page of candidates.
#[must_use]
pub fn queue(
    filter: &CandidateQuery,
    candidates: &Candidates,
    deferred: Option<&Read<()>>,
) -> AnyView {
    stack(view! {
        {deferred_notice(deferred)}
        {card(tr(Msg::Filters), filters(filter))}
        {list(filter, candidates)}
    })
    .into_any()
}

/// The outcome of a deferral just posted.
fn deferred_notice(deferred: Option<&Read<()>>) -> Option<AnyView> {
    deferred.map(|deferred| match deferred {
        Ok(()) => notice(
            Tone::Ok,
            Announce::Status,
            None,
            view! { <p>{tr(Msg::QueueDeferred)}</p> },
        )
        .into_any(),
        Err(probe) => server_state(
            Some(probe),
            Needs {
                what: Msg::WhatDecision,
                away: false,
            },
            "POST /operator/candidates/{id}/decide",
        ),
    })
}

/// The hidden fields that carry `filter` with another form of the page.
fn kept(filter: &CandidateQuery) -> AnyView {
    let grade = filter.grade.map(grade_key).unwrap_or_default();
    let domain = filter.domain.clone().unwrap_or_default();
    let days = filter
        .older_than_days
        .map(|days| days.to_string())
        .unwrap_or_default();
    view! {
        <input type="hidden" name="grade" value=grade />
        <input type="hidden" name="domain" value=domain />
        <input type="hidden" name="older" value=days />
        {filter.deferred.then(|| view! { <input type="hidden" name="deferred" value="on" /> })}
    }
    .into_any()
}

/// The filter form, its fields holding `filter`.
fn filters(filter: &CandidateQuery) -> AnyView {
    let chosen = filter.grade;
    let grades = [
        None,
        Some(Grade::Certain),
        Some(Grade::Probable),
        Some(Grade::Possible),
    ]
    .into_iter()
    .map(|grade| {
        let value = grade.map(grade_key).unwrap_or_default();
        let words = grade.map_or_else(|| tr(Msg::QueueAnyGrade), grade_words);
        view! { <option value=value selected=grade == chosen>{words}</option> }
    })
    .collect_view();
    let days = filter
        .older_than_days
        .map(|days| days.to_string())
        .unwrap_or_default();
    view! {
        <form method="post" action=QUEUE class="fui-form">
            <input type="hidden" name=FORM value=FILTER />
            <div class="fui-form-grid">
                {Field::new("grade", tr(Msg::ColGrade)).view(view! {
                    <select id="grade" name="grade" class=SELECT>{grades}</select>
                })}
                {Field::new("domain", tr(Msg::QueueDomain)).view(view! {
                    <input id="domain" name="domain" type="text" class=INPUT autocomplete="off" value=filter.domain.clone().unwrap_or_default() />
                })}
                {Field::new("older", tr(Msg::QueueOlder)).view(view! {
                    <input id="older" name="older" type="number" min="0" class=INPUT value=days />
                })}
            </div>
            {check_field(
                "deferred",
                tr(Msg::QueueShowDeferred),
                tr(Msg::QueueShowDeferredHelp),
                view! { <input id="deferred" name="deferred" type="checkbox" value="on" checked=filter.deferred aria-describedby=help_id("deferred") /> },
            )}
            <div class="fui-form-foot">
                <button type="submit" class=Variant::Primary.class()>
                    {label(Some(Glyph::LIST_FILTER), tr(Msg::ApplyFilters))}
                </button>
                <button type="submit" name="clear" value="on" formnovalidate class=Variant::Secondary.class()>
                    {label(Some(Glyph::X), tr(Msg::QueueClear))}
                </button>
            </div>
        </form>
    }
    .into_any()
}

/// Whether `filter` narrows the queue.
fn filtered(filter: &CandidateQuery) -> bool {
    filter.grade.is_some()
        || filter
            .domain
            .as_deref()
            .is_some_and(|domain| !domain.is_empty())
        || filter.older_than_days.is_some()
        || filter.deferred
}

/// The page of candidates in the work-list, or the queue's empty state.
fn list(filter: &CandidateQuery, candidates: &Candidates) -> AnyView {
    if candidates.rows.is_empty() {
        let body = if filtered(filter) {
            list_empty(tr(Msg::QueueNoMatch), None)
        } else {
            let last = candidates.last_decision_at.as_ref().map(|at| {
                view! {
                    <p class="fui-small fui-muted">
                        {tr(Msg::QueueLastDecision)} " " {timestamp(at)}
                    </p>
                }
                .into_any()
            });
            view! {
                {list_empty(tr(Msg::QueueEmpty), None)}
                {last}
                <p class="fui-small"><a href=DASHBOARD>{tr(Msg::Dashboard)}</a></p>
            }
            .into_any()
        };
        return list_card(ListCard {
            title: tr(Msg::Queue),
            count: Some(candidates.total.into()),
            order: None,
            tabs: None,
            actions: None,
            body,
            pager: None,
        });
    }
    let columns = [
        Column::text("pair", tr(Msg::ColPair)).role(Role::Title),
        Column::text("born", tr(Msg::BirthDate)).role(Role::Sub),
        Column::text("domains", tr(Msg::ColDomains)).role(Role::Wide),
        Column::text("score", tr(Msg::ColScore)).numeric(),
        Column::text("grade", tr(Msg::ColGrade)),
        Column::text("age", tr(Msg::ColWaiting)).role(Role::Quiet),
        Column::text("open", tr(Msg::QueueGlance)).role(Role::Wide),
    ];
    let [_, born, domains, score, grade, age, open] = &columns;
    let rows = candidates
        .rows
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let class = (candidate.status == Status::Deferred)
                .then_some(ferrohealth_ui::table::view::EXCLUDED);
            row(WorkRow {
                key: &candidate.id,
                focused: index == 0,
                class,
                cells: view! {
                    {title_cell(Some(&format!("{QUEUE}/{}", candidate.id)), pair_name(candidate), None)}
                    {cell(born, pair_line(candidate.a.birth_date.as_ref(), candidate.b.birth_date.as_ref()))}
                    {cell(domains, pair_line(Some(&candidate.a.domain.name), Some(&candidate.b.domain.name)))}
                    {cell(score, score_bar(candidate.score, grade_tone(candidate.grade)))}
                    {cell(grade, pill(grade_tone(candidate.grade), &grade_words(candidate.grade)))}
                    {cell(age, waited(candidate))}
                    {cell(open, glance(filter, candidate))}
                }
                .into_any(),
            })
        })
        .collect_view()
        .into_any();
    let table = data_table(DataTable {
        caption: &tr(Msg::Queue),
        columns: &columns,
        sorting: Sorting::default(),
        body: Body::Rows(rows),
    })
    .into_any();
    let first = u64::from(candidates.offset).saturating_add(1);
    let shown = u64::try_from(candidates.rows.len()).unwrap_or(u64::MAX);
    let range = tr_with(
        Msg::QueueRange,
        &[
            ("first", first.into()),
            ("last", first.saturating_add(shown).saturating_sub(1).into()),
            ("total", candidates.total.into()),
        ],
    );
    let hint =
        view! { <p class="fui-small fui-muted">{range} " · " {tr(Msg::QueueKeys)}</p> }.into_any();
    let foot = worklist_foot(Some(hint), pager(filter, candidates));
    list_card(ListCard {
        title: tr(Msg::Queue),
        count: Some(candidates.total.into()),
        order: None,
        tabs: None,
        actions: None,
        body: view! {
            <WorkList>{table}</WorkList>
            {foot}
        }
        .into_any(),
        pager: None,
    })
}

/// The two leading names of a candidate, or the words for none.
fn pair_name(candidate: &CandidateRow) -> String {
    let name = |side: &Side| side.name.clone().unwrap_or_else(|| tr(Msg::NoName));
    tr_with(
        Msg::PairNames,
        &[
            ("a", name(&candidate.a).into()),
            ("b", name(&candidate.b).into()),
        ],
    )
}

/// Two values one above the other, a dash for one that is absent.
fn pair_line(a: Option<&String>, b: Option<&String>) -> AnyView {
    let one = |value: Option<&String>| {
        value
            .cloned()
            .map_or_else(|| or_none(String::new()), IntoAny::into_any)
    };
    view! {
        <span>{one(a)}<br />{one(b)}</span>
    }
    .into_any()
}

/// When a candidate was queued, and for a deferred one when it comes back.
fn waited(candidate: &CandidateRow) -> AnyView {
    match &candidate.deferred_until {
        Some(until) => view! {
            <span>{tr(Msg::QueueBackOn)} " " {timestamp(until)}</span>
        }
        .into_any(),
        None => timestamp(&candidate.created_at).into_any(),
    }
}

/// The side panel of a row: the pair at a glance, Open to decide and the
/// deferral, opened by `o` or `d`; with no WebAssembly its opener leads to
/// the pair's own page.
fn glance(filter: &CandidateQuery, candidate: &CandidateRow) -> AnyView {
    let id = candidate.id.clone();
    let panel = format!("glance-{id}");
    let until_id = format!("until-{id}");
    let reason_id = format!("reason-{id}");
    let facts = definitions(vec![
        (tr(Msg::ColPair), pair_name(candidate).into_any()),
        (
            tr(Msg::ColScore),
            score_bar(candidate.score, grade_tone(candidate.grade)),
        ),
        (
            tr(Msg::ColGrade),
            pill(grade_tone(candidate.grade), &grade_words(candidate.grade)).into_any(),
        ),
        (
            tr(Msg::ColDomains),
            pair_line(
                Some(&candidate.a.domain.name),
                Some(&candidate.b.domain.name),
            ),
        ),
    ]);
    let body = view! {
        {facts}
        <p><a href=format!("{QUEUE}/{id}") class=Variant::Primary.class()>{tr(Msg::QueueDecide)}</a></p>
        <form method="post" action=QUEUE class="fui-form">
            <input type="hidden" name=FORM value=DEFER />
            <input type="hidden" name="candidate" value=id.clone() />
            {kept(filter)}
            {Field::new(until_id.clone(), tr(Msg::DeferUntil)).view(view! {
                <input id=until_id.clone() name="until" type="date" class=INPUT required />
            })}
            {Field::new(reason_id.clone(), tr(Msg::RevealReason)).view(view! {
                <input id=reason_id.clone() name="reason" type="text" class=INPUT required maxlength="500" autocomplete="off" />
            })}
            <div class="fui-form-foot">
                <button type="submit" class=Variant::Secondary.class()>{label(Some(Glyph::CLOCK), tr(Msg::Defer))}</button>
            </div>
        </form>
    }
    .into_any();
    view! {
        <SidePanel
            id=panel
            title=pair_name(candidate)
            opener=Opener {
                label: tr(Msg::QueueGlance),
                glyph: Some(Glyph::PANEL_RIGHT),
                variant: Variant::Ghost,
                href: format!("{QUEUE}/{}", candidate.id),
            }
            words=SidePanelWords::of_page()
        >
            {body}
        </SidePanel>
    }
    .into_any()
}

/// The posted paging of the queue, the filters kept.
fn pager(filter: &CandidateQuery, candidates: &Candidates) -> Option<AnyView> {
    let previous = (candidates.offset > 0).then(|| candidates.offset.saturating_sub(PAGE));
    if previous.is_none() && candidates.next.is_none() {
        return None;
    }
    let page = |offset: u32, words: Msg, glyph: Glyph| {
        view! {
            <form method="post" action=QUEUE class="fui-inline-form">
                <input type="hidden" name=FORM value=FILTER />
                {kept(filter)}
                <input type="hidden" name="offset" value=offset.to_string() />
                <button type="submit" class=Variant::Secondary.class()>{label(Some(glyph), tr(words))}</button>
            </form>
        }
    };
    Some(
        view! {
            <div class="fui-actions">
                {previous.map(|offset| page(offset, Msg::QueuePrevious, Glyph::CHEVRON_LEFT))}
                {candidates.next.map(|offset| page(offset, Msg::QueueNext, Glyph::CHEVRON_RIGHT))}
            </div>
        }
        .into_any(),
    )
}
