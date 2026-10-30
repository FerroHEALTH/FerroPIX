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

use ferrohealth_ui::html::Html;
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::table::view::{
    ACTIONS, Body, Column, DataTable, EXCLUDED, Role, Sorting, cell, data_table, title_cell,
};
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::form::{Field, INPUT, SELECT, check_field, help_id};
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::score::score_bar;
use ferrohealth_ui::view::side_panel::{Opener, SidePanel, SidePanelWords, side_panel};
use ferrohealth_ui::view::status::pill;
use ferrohealth_ui::view::surface::{card, definitions, stack};
use ferrohealth_ui::view::template::list::{ListCard, list_card, list_empty};
use ferrohealth_ui::view::time::timestamp;
use ferrohealth_ui::view::worklist::{WorkRow, row, worklist, worklist_foot};
use ferropix_operator_api::candidate::{
    CandidateQuery, CandidateRow, Candidates, PAGE, Side, Status,
};
use ferropix_operator_api::patient::Grade;

use crate::app::{DASHBOARD, QUEUE};
use crate::locale::messages::Msg;
use crate::markup::{h, hidden, t};
use crate::model::Read;
use crate::pages::{Needs, or_dash, server_state};
use crate::visit::Visit;

/// The hidden field that names which of the queue's forms was posted.
pub const FORM: &str = "form";

/// The value of [`FORM`] for the filters and the paging.
pub const FILTER: &str = "filter";

/// The value of [`FORM`] for a deferral from a row's side panel.
pub const DEFER: &str = "defer";

/// The words of a grade.
#[must_use]
pub fn grade_words(visit: &Visit, grade: Grade) -> String {
    visit.tr(match grade {
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
    visit: &Visit,
    filter: &CandidateQuery,
    candidates: &Candidates,
    deferred: Option<&Read<()>>,
) -> Html {
    stack(Html::join([
        deferred_notice(visit, deferred),
        card(visit.tr(Msg::Filters), filters(visit, filter)),
        list(visit, filter, candidates),
    ]))
}

/// The outcome of a deferral just posted.
fn deferred_notice(visit: &Visit, deferred: Option<&Read<()>>) -> Html {
    match deferred {
        None => Html::empty(),
        Some(Ok(())) => notice(
            Tone::Ok,
            Announce::Status,
            None,
            h(format!("<p>{}</p>", t(visit.tr(Msg::QueueDeferred)))),
        ),
        Some(Err(probe)) => server_state(
            visit,
            Some(probe),
            Needs::of(Msg::WhatDecision),
            "POST /operator/candidates/{id}/decide",
        ),
    }
}

/// The hidden fields that carry `filter` with another form of the page.
fn kept(filter: &CandidateQuery) -> Html {
    let grade = filter.grade.map(grade_key).unwrap_or_default();
    let domain = filter.domain.clone().unwrap_or_default();
    let days = filter
        .older_than_days
        .map(|days| days.to_string())
        .unwrap_or_default();
    Html::join([
        hidden("grade", grade),
        hidden("domain", &domain),
        hidden("older", &days),
        if filter.deferred {
            hidden("deferred", "on")
        } else {
            Html::empty()
        },
    ])
}

/// The filter form, its fields holding `filter`.
fn filters(visit: &Visit, filter: &CandidateQuery) -> Html {
    let chosen = filter.grade;
    let grades: Html = [
        None,
        Some(Grade::Certain),
        Some(Grade::Probable),
        Some(Grade::Possible),
    ]
    .into_iter()
    .map(|grade| {
        let value = grade.map(grade_key).unwrap_or_default();
        let words = grade.map_or_else(
            || visit.tr(Msg::QueueAnyGrade),
            |grade| grade_words(visit, grade),
        );
        h(format!(
            "<option value=\"{}\"{}>{}</option>",
            t(value),
            if grade == chosen { " selected" } else { "" },
            t(words)
        ))
    })
    .collect();
    let days = filter
        .older_than_days
        .map(|days| days.to_string())
        .unwrap_or_default();
    let grade = Field::new("grade", visit.tr(Msg::ColGrade)).view(h(format!(
        "<select id=\"grade\" name=\"grade\" class=\"{SELECT}\">{grades}</select>"
    )));
    let domain = Field::new("domain", visit.tr(Msg::QueueDomain)).view(h(format!(
        "<input id=\"domain\" name=\"domain\" type=\"text\" class=\"{INPUT}\" autocomplete=\"off\" value=\"{}\">",
        t(filter.domain.clone().unwrap_or_default())
    )));
    let older = Field::new("older", visit.tr(Msg::QueueOlder)).view(h(format!(
        "<input id=\"older\" name=\"older\" type=\"number\" min=\"0\" class=\"{INPUT}\" value=\"{}\">",
        t(days)
    )));
    let deferred = check_field(
        "deferred",
        visit.tr(Msg::QueueShowDeferred),
        visit.tr(Msg::QueueShowDeferredHelp),
        h(format!(
            "<input id=\"deferred\" name=\"deferred\" type=\"checkbox\" value=\"on\"{} aria-describedby=\"{}\">",
            if filter.deferred { " checked" } else { "" },
            t(help_id("deferred"))
        )),
    );
    h(format!(
        "<form method=\"post\" action=\"{QUEUE}\" class=\"fui-form\">{}<div class=\"fui-form-grid\">{grade}{domain}{older}</div>{deferred}<div class=\"fui-form-foot\"><button type=\"submit\" class=\"{}\">{}</button><button type=\"submit\" name=\"clear\" value=\"on\" formnovalidate class=\"{}\">{}</button></div></form>",
        hidden(FORM, FILTER),
        Variant::Primary.class(),
        label(Some(Glyph::LIST_FILTER), visit.tr(Msg::ApplyFilters)),
        Variant::Secondary.class(),
        label(Some(Glyph::X), visit.tr(Msg::QueueClear)),
    ))
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

/// The list card of the queue holding `body`.
fn queue_card(visit: &Visit, candidates: &Candidates, body: Html) -> Html {
    list_card(
        &visit.locale,
        ListCard {
            title: visit.tr(Msg::Queue),
            count: Some(candidates.total.into()),
            order: None,
            tabs: None,
            actions: None,
            body,
            pager: None,
        },
    )
}

/// The queue's empty state: no pair matches the filter, or none waits.
fn empty(visit: &Visit, filter: &CandidateQuery, candidates: &Candidates) -> Html {
    if filtered(filter) {
        return list_empty(visit.tr(Msg::QueueNoMatch), None);
    }
    let last = candidates
        .last_decision_at
        .as_ref()
        .map_or_else(Html::empty, |at| {
            h(format!(
                "<p class=\"fui-small fui-muted\">{} {}</p>",
                t(visit.tr(Msg::QueueLastDecision)),
                timestamp(&visit.locale, at)
            ))
        });
    Html::join([
        list_empty(visit.tr(Msg::QueueEmpty), None),
        last,
        h(format!(
            "<p class=\"fui-small\"><a href=\"{DASHBOARD}\">{}</a></p>",
            t(visit.tr(Msg::Dashboard))
        )),
    ])
}

/// The queue's columns.
fn columns(visit: &Visit) -> [Column; 7] {
    [
        Column::text("pair", visit.tr(Msg::ColPair)).role(Role::Title),
        Column::text("born", visit.tr(Msg::BirthDate)).role(Role::Sub),
        Column::text("domains", visit.tr(Msg::ColDomains)).role(Role::Wide),
        Column::text("score", visit.tr(Msg::ColScore)).numeric(),
        Column::text("grade", visit.tr(Msg::ColGrade)),
        Column::text("age", visit.tr(Msg::ColWaiting)).role(Role::Quiet),
        Column::text("open", visit.tr(Msg::QueueGlance)).role(Role::Wide),
    ]
}

/// The work-list row of `candidate` under `columns`, focused when it is
/// the first.
fn candidate_row(
    visit: &Visit,
    filter: &CandidateQuery,
    columns: &[Column; 7],
    first: bool,
    candidate: &CandidateRow,
) -> Html {
    let [_, born, domains, score, grade, age, open] = columns;
    let class = (candidate.status == Status::Deferred).then_some(EXCLUDED);
    row(WorkRow {
        key: &candidate.id,
        focused: first,
        class,
        cells: Html::join([
            title_cell(
                Some(&format!("{QUEUE}/{}", candidate.id)),
                t(pair_name(visit, candidate)),
                None,
            ),
            cell(
                born,
                pair_line(
                    visit,
                    candidate.a.birth_date.as_ref(),
                    candidate.b.birth_date.as_ref(),
                ),
            ),
            cell(
                domains,
                pair_line(
                    visit,
                    Some(&candidate.a.domain.name),
                    Some(&candidate.b.domain.name),
                ),
            ),
            cell(
                score,
                score_bar(&visit.locale, candidate.score, grade_tone(candidate.grade)),
            ),
            cell(
                grade,
                pill(
                    grade_tone(candidate.grade),
                    &grade_words(visit, candidate.grade),
                ),
            ),
            cell(age, waited(visit, candidate)),
            actions_cell(open, &glance(visit, filter, candidate)),
        ]),
    })
}

/// The cell of `column` holding the row's actions, `actions`.
// NOTE: FerroUI COMPONENTS.md R9: the row is its link's target, so its actions sit in the
// actions cell (`table::view::ACTIONS`), above the link.
fn actions_cell(column: &Column, actions: &Html) -> Html {
    h(format!(
        "<td data-label=\"{}\" data-role=\"{}\" class=\"{ACTIONS}\">{actions}</td>",
        t(column.label.text()),
        column.role.key()
    ))
}

/// The page of candidates in the work-list, or the queue's empty state.
fn list(visit: &Visit, filter: &CandidateQuery, candidates: &Candidates) -> Html {
    if candidates.rows.is_empty() {
        return queue_card(visit, candidates, empty(visit, filter, candidates));
    }
    let columns = columns(visit);
    let rows: Html = candidates
        .rows
        .iter()
        .enumerate()
        .map(|(index, candidate)| candidate_row(visit, filter, &columns, index == 0, candidate))
        .collect();
    let caption = visit.tr(Msg::Queue);
    let table = data_table(DataTable {
        caption: &caption,
        columns: &columns,
        sorting: Sorting::default(),
        body: Body::Rows(rows),
    });
    let first = u64::from(candidates.offset).saturating_add(1);
    let shown = u64::try_from(candidates.rows.len()).unwrap_or(u64::MAX);
    let range = visit.tr_with(
        Msg::QueueRange,
        &[
            ("first", first.into()),
            ("last", first.saturating_add(shown).saturating_sub(1).into()),
            ("total", candidates.total.into()),
        ],
    );
    let hint = h(format!(
        "<p class=\"fui-small fui-muted\">{} · {}</p>",
        t(range),
        t(visit.tr(Msg::QueueKeys))
    ));
    let foot = worklist_foot(Some(hint), pager(visit, filter, candidates));
    queue_card(visit, candidates, worklist(table).then(foot))
}

/// The two leading names of a candidate, or the words for none.
fn pair_name(visit: &Visit, candidate: &CandidateRow) -> String {
    let name = |side: &Side| side.name.clone().unwrap_or_else(|| visit.tr(Msg::NoName));
    visit.tr_with(
        Msg::PairNames,
        &[
            ("a", name(&candidate.a).into()),
            ("b", name(&candidate.b).into()),
        ],
    )
}

/// Two values one above the other, a dash for one that is absent.
fn pair_line(visit: &Visit, a: Option<&String>, b: Option<&String>) -> Html {
    h(format!(
        "<span>{}<br>{}</span>",
        or_dash(&visit.locale, a.cloned()),
        or_dash(&visit.locale, b.cloned())
    ))
}

/// When a candidate was queued, and for a deferred one when it comes back.
fn waited(visit: &Visit, candidate: &CandidateRow) -> Html {
    match &candidate.deferred_until {
        Some(until) => h(format!(
            "<span>{} {}</span>",
            t(visit.tr(Msg::QueueBackOn)),
            timestamp(&visit.locale, until)
        )),
        None => timestamp(&visit.locale, &candidate.created_at),
    }
}

/// The side panel of a row: the pair at a glance, Open to decide and the
/// deferral; with no script its opener leads to the pair's own page.
fn glance(visit: &Visit, filter: &CandidateQuery, candidate: &CandidateRow) -> Html {
    let locale = &visit.locale;
    let id = &candidate.id;
    let until_id = format!("until-{id}");
    let reason_id = format!("reason-{id}");
    let facts = definitions(vec![
        (visit.tr(Msg::ColPair), t(pair_name(visit, candidate))),
        (
            visit.tr(Msg::ColScore),
            score_bar(locale, candidate.score, grade_tone(candidate.grade)),
        ),
        (
            visit.tr(Msg::ColGrade),
            pill(
                grade_tone(candidate.grade),
                &grade_words(visit, candidate.grade),
            ),
        ),
        (
            visit.tr(Msg::ColDomains),
            pair_line(
                visit,
                Some(&candidate.a.domain.name),
                Some(&candidate.b.domain.name),
            ),
        ),
    ]);
    let until = Field::new(until_id.clone(), visit.tr(Msg::DeferUntil)).view(h(format!(
        "<input id=\"{}\" name=\"until\" type=\"date\" class=\"{INPUT}\" required>",
        t(&until_id)
    )));
    let reason = Field::new(reason_id.clone(), visit.tr(Msg::RevealReason)).view(h(format!(
        "<input id=\"{}\" name=\"reason\" type=\"text\" class=\"{INPUT}\" required maxlength=\"500\" autocomplete=\"off\">",
        t(&reason_id)
    )));
    let body = h(format!(
        "{facts}<p><a href=\"{}\" class=\"{}\">{}</a></p><form method=\"post\" action=\"{QUEUE}\" class=\"fui-form\">{}{}{}{until}{reason}<div class=\"fui-form-foot\"><button type=\"submit\" class=\"{}\">{}</button></div></form>",
        t(format!("{QUEUE}/{id}")),
        Variant::Primary.class(),
        t(visit.tr(Msg::QueueDecide)),
        hidden(FORM, DEFER),
        hidden("candidate", id),
        kept(filter),
        Variant::Secondary.class(),
        label(Some(Glyph::CLOCK), visit.tr(Msg::Defer)),
    ));
    side_panel(SidePanel {
        id: format!("glance-{id}"),
        title: pair_name(visit, candidate),
        opener: Opener {
            label: visit.tr(Msg::QueueGlance),
            glyph: Some(Glyph::PANEL_RIGHT),
            variant: Variant::Ghost,
            href: format!("{QUEUE}/{id}"),
        },
        body,
        words: SidePanelWords::of(locale),
    })
}

/// The posted paging of the queue, the filters kept.
fn pager(visit: &Visit, filter: &CandidateQuery, candidates: &Candidates) -> Option<Html> {
    let previous = (candidates.offset > 0).then(|| candidates.offset.saturating_sub(PAGE));
    if previous.is_none() && candidates.next.is_none() {
        return None;
    }
    let page = |offset: u32, words: Msg, glyph: Glyph| {
        h(format!(
            "<form method=\"post\" action=\"{QUEUE}\" class=\"fui-inline-form\">{}{}{}<button type=\"submit\" class=\"{}\">{}</button></form>",
            hidden(FORM, FILTER),
            kept(filter),
            hidden("offset", &offset.to_string()),
            Variant::Secondary.class(),
            label(Some(glyph), visit.tr(words)),
        ))
    };
    Some(h(format!(
        "<div class=\"fui-actions\">{}{}</div>",
        previous.map_or_else(Html::empty, |offset| page(
            offset,
            Msg::QueuePrevious,
            Glyph::CHEVRON_LEFT
        )),
        candidates.next.map_or_else(Html::empty, |offset| page(
            offset,
            Msg::QueueNext,
            Glyph::CHEVRON_RIGHT
        )),
    )))
}
