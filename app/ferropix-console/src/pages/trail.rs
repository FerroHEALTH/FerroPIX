// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The audit trail at `/audit` (design 13): the steward actions of
//! `GET /operator/actions`, newest first, under the filters a form posts.
//!
//! Every filter is a form field and is posted, so the filter and the page
//! stay out of the address; the next page is a post too, carrying the
//! cursor the server named. The export posts the same filter and answers
//! the server's CSV as a file, itself an audited action. No specification
//! governs the console: our own design (decision D8: the trail reads the
//! server's own `steward_action` table).

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::table::view::{
    Body, Column, DataTable, Role, Sorting, cell, data_table, title_cell,
};
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::form::{Field, INPUT};
use ferrohealth_ui::view::status::badge;
use ferrohealth_ui::view::surface::{card, definitions, disclosure, or_none, stack};
use ferrohealth_ui::view::template::list::{ListCard, list_card, list_empty};
use ferrohealth_ui::view::time::timestamp;
use ferropix_operator_api::actions::{Action, ActionFilter, Actions};
use leptos::prelude::*;

use crate::app::AUDIT;
use crate::locale::{Msg, tr};

/// The route the export posts to.
pub const EXPORT: &str = "/audit/export";

/// The tone of an action's kind.
fn tone(kind: &str) -> Tone {
    match kind {
        "reveal" | "export" | "pseudonym-resolve" => Tone::Warn,
        "merge" | "unmerge" | "decide" => Tone::Info,
        "domain-create" | "domain-update" => Tone::Accent,
        _ => Tone::Neutral,
    }
}

/// A text field of the filter form, holding `value`.
fn filter_field(
    id: &'static str,
    words: Msg,
    value: Option<&String>,
    kind: &'static str,
) -> AnyView {
    Field::new(id, tr(words)).view(view! {
        <input id=id name=id type=kind class=INPUT autocomplete="off" value=value.cloned().unwrap_or_default() />
    })
}

/// The filter form, its fields holding `filter`.
fn filters(filter: &ActionFilter) -> AnyView {
    view! {
        <form method="post" action=AUDIT class="fui-form">
            <div class="fui-form-grid">
                {filter_field("actor", Msg::ColActor, filter.actor.as_ref(), "text")}
                {filter_field("kind", Msg::ColKind, filter.kind.as_ref(), "text")}
                {filter_field("person", Msg::ColPerson, filter.person.as_ref(), "text")}
                {filter_field("from", Msg::FilterFrom, filter.from.as_ref(), "date")}
                {filter_field("to", Msg::FilterTo, filter.to.as_ref(), "date")}
            </div>
            <div class="fui-form-foot">
                <button type="submit" class=Variant::Primary.class()>
                    {label(Some(Glyph::LIST_FILTER), tr(Msg::ApplyFilters))}
                </button>
                <button type="submit" formaction=EXPORT class=Variant::Secondary.class()>
                    {label(Some(Glyph::DOWNLOAD), tr(Msg::ExportCsv))}
                </button>
            </div>
        </form>
    }
    .into_any()
}

/// The trail: its filters, then its page of actions.
#[must_use]
pub fn trail(filter: &ActionFilter, actions: &Actions) -> AnyView {
    stack(view! {
        {card(tr(Msg::Filters), filters(filter))}
        {table(filter, actions)}
    })
    .into_any()
}

/// The page of actions, with the next page's post when one follows.
fn table(filter: &ActionFilter, actions: &Actions) -> AnyView {
    if actions.actions.is_empty() {
        return list_card(ListCard {
            title: tr(Msg::Audit),
            count: None,
            order: None,
            tabs: None,
            actions: None,
            body: list_empty(tr(Msg::TrailEmpty), None),
            pager: None,
        });
    }
    let columns = [
        Column::text("at", tr(Msg::ColTime)).role(Role::Title),
        Column::text("actor", tr(Msg::ColActor)).role(Role::Sub),
        Column::text("kind", tr(Msg::ColKind)),
        Column::text("person", tr(Msg::ColPerson)).role(Role::Wide),
        Column::text("reason", tr(Msg::ColReason)).role(Role::Wide),
        Column::text("detail", tr(Msg::ColDetail)).role(Role::Wide),
    ];
    let [_, actor, kind, person, reason, detail] = &columns;
    let rows = actions
        .actions
        .iter()
        .map(|action| {
            view! {
                <tr>
                    {title_cell(None, timestamp(&action.at), None)}
                    {cell(actor, action.actor.clone())}
                    {cell(kind, badge(tone(&action.kind), crate::pages::action_words(&action.kind)))}
                    {cell(person, person_link(action))}
                    {cell(reason, action.reason.clone().unwrap_or_default())}
                    {cell(detail, details(action))}
                </tr>
            }
        })
        .collect_view()
        .into_any();
    let pager = actions.next.clone().map(|next| {
        let hidden = [
            ("actor", filter.actor.clone()),
            ("kind", filter.kind.clone()),
            ("person", filter.person.clone()),
            ("from", filter.from.clone()),
            ("to", filter.to.clone()),
            ("after", Some(next)),
        ]
        .into_iter()
        .filter_map(|(name, value)| {
            value.map(|value| view! { <input type="hidden" name=name value=value /> })
        })
        .collect_view();
        view! {
            <form method="post" action=AUDIT>
                {hidden}
                <button type="submit" class=Variant::Secondary.class()>
                    {label(Some(Glyph::CHEVRON_RIGHT), tr(Msg::OlderActions))}
                </button>
            </form>
        }
        .into_any()
    });
    list_card(ListCard {
        title: tr(Msg::Audit),
        count: None,
        order: None,
        tabs: None,
        actions: None,
        body: data_table(DataTable {
            caption: &tr(Msg::Audit),
            columns: &columns,
            sorting: Sorting::default(),
            body: Body::Rows(rows),
        })
        .into_any(),
        pager,
    })
}

/// The person an action concerns, as a link to the patient.
fn person_link(action: &Action) -> AnyView {
    action.person.clone().map_or_else(
        || or_none(String::new()),
        |person| {
            view! { <a href=format!("/patients/{person}")><code>{person.clone()}</code></a> }
                .into_any()
        },
    )
}

/// The disclosure of an action's other references.
fn details(action: &Action) -> AnyView {
    let mut rows: Vec<(String, AnyView)> = Vec::new();
    for (words, value) in [
        (Msg::DetailRecord, &action.record),
        (Msg::DetailCandidate, &action.candidate),
        (Msg::DetailMerge, &action.merge),
        (Msg::DetailDomain, &action.domain),
        (Msg::DetailPseudonymDomain, &action.pseudonym_domain),
        (Msg::DetailRequestId, &action.request_id),
    ] {
        if let Some(value) = value {
            rows.push((tr(words), view! { <code>{value.clone()}</code> }.into_any()));
        }
    }
    rows.push((
        tr(Msg::DetailAction),
        view! { <code>{action.id.clone()}</code> }.into_any(),
    ));
    disclosure(tr(Msg::ActionDetail), definitions(rows)).into_any()
}
