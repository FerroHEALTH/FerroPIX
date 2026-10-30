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

use ferrohealth_ui::html::Html;
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::table::view::{
    Body, Column, DataTable, Role, Sorting, cell, data_table, title_cell,
};
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::form::{Field, INPUT};
use ferrohealth_ui::view::status::badge;
use ferrohealth_ui::view::surface::{card, definitions, disclosure, stack};
use ferrohealth_ui::view::template::list::{ListCard, list_card, list_empty};
use ferrohealth_ui::view::time::timestamp;
use ferropix_operator_api::actions::{Action, ActionFilter, Actions};

use crate::app::AUDIT;
use crate::locale::messages::Msg;
use crate::markup::{h, hidden, t};
use crate::pages::or_dash;
use crate::visit::Visit;

/// The route the export posts to.
pub const EXPORT: &str = "/audit/export";

/// The input type of a text filter.
const TEXT: &str = "text";

/// The input type of a date filter.
const DATE: &str = "date";

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
    visit: &Visit,
    id: &'static str,
    words: Msg,
    value: Option<&String>,
    kind: &'static str,
) -> Html {
    Field::new(id, visit.tr(words)).view(h(format!(
        "<input id=\"{id}\" name=\"{id}\" type=\"{kind}\" class=\"{INPUT}\" autocomplete=\"off\" value=\"{}\">",
        t(value.cloned().unwrap_or_default())
    )))
}

/// The filter form, its fields holding `filter`.
fn filters(visit: &Visit, filter: &ActionFilter) -> Html {
    h(format!(
        "<form method=\"post\" action=\"{AUDIT}\" class=\"fui-form\"><div class=\"fui-form-grid\">{}{}{}{}{}</div><div class=\"fui-form-foot\"><button type=\"submit\" class=\"{}\">{}</button><button type=\"submit\" formaction=\"{EXPORT}\" class=\"{}\">{}</button></div></form>",
        filter_field(visit, "actor", Msg::ColActor, filter.actor.as_ref(), TEXT),
        filter_field(visit, "kind", Msg::ColKind, filter.kind.as_ref(), TEXT),
        filter_field(
            visit,
            "person",
            Msg::ColPerson,
            filter.person.as_ref(),
            TEXT
        ),
        filter_field(visit, "from", Msg::FilterFrom, filter.from.as_ref(), DATE),
        filter_field(visit, "to", Msg::FilterTo, filter.to.as_ref(), DATE),
        Variant::Primary.class(),
        label(Some(Glyph::LIST_FILTER), visit.tr(Msg::ApplyFilters)),
        Variant::Secondary.class(),
        label(Some(Glyph::DOWNLOAD), visit.tr(Msg::ExportCsv)),
    ))
}

/// The trail: its filters, then its page of actions.
#[must_use]
pub fn trail(visit: &Visit, filter: &ActionFilter, actions: &Actions) -> Html {
    stack(card(visit.tr(Msg::Filters), filters(visit, filter)).then(table(visit, filter, actions)))
}

/// The page of actions, with the next page's post when one follows.
fn table(visit: &Visit, filter: &ActionFilter, actions: &Actions) -> Html {
    let locale = &visit.locale;
    if actions.actions.is_empty() {
        return list_card(
            locale,
            ListCard {
                title: visit.tr(Msg::Audit),
                count: None,
                order: None,
                tabs: None,
                actions: None,
                body: list_empty(visit.tr(Msg::TrailEmpty), None),
                pager: None,
            },
        );
    }
    let columns = [
        Column::text("at", visit.tr(Msg::ColTime)).role(Role::Title),
        Column::text("actor", visit.tr(Msg::ColActor)).role(Role::Sub),
        Column::text("kind", visit.tr(Msg::ColKind)),
        Column::text("person", visit.tr(Msg::ColPerson)).role(Role::Wide),
        Column::text("reason", visit.tr(Msg::ColReason)).role(Role::Wide),
        Column::text("detail", visit.tr(Msg::ColDetail)).role(Role::Wide),
    ];
    let [_, actor, kind, person, reason, detail] = &columns;
    let rows: Html = actions
        .actions
        .iter()
        .map(|action| {
            h(format!(
                "<tr>{}{}{}{}{}{}</tr>",
                title_cell(None, timestamp(locale, &action.at), None),
                cell(actor, t(&action.actor)),
                cell(
                    kind,
                    badge(
                        tone(&action.kind),
                        crate::pages::action_words(locale, &action.kind)
                    )
                ),
                cell(person, person_link(visit, action)),
                cell(reason, t(action.reason.clone().unwrap_or_default())),
                cell(detail, details(visit, action)),
            ))
        })
        .collect();
    let pager = actions.next.as_ref().map(|next| {
        let kept: Html = [
            ("actor", filter.actor.as_ref()),
            ("kind", filter.kind.as_ref()),
            ("person", filter.person.as_ref()),
            ("from", filter.from.as_ref()),
            ("to", filter.to.as_ref()),
            ("after", Some(next)),
        ]
        .into_iter()
        .filter_map(|(name, value)| value.map(|value| hidden(name, value)))
        .collect();
        h(format!(
            "<form method=\"post\" action=\"{AUDIT}\">{kept}<button type=\"submit\" class=\"{}\">{}</button></form>",
            Variant::Secondary.class(),
            label(Some(Glyph::CHEVRON_RIGHT), visit.tr(Msg::OlderActions)),
        ))
    });
    let caption = visit.tr(Msg::Audit);
    list_card(
        locale,
        ListCard {
            title: visit.tr(Msg::Audit),
            count: None,
            order: None,
            tabs: None,
            actions: None,
            body: data_table(DataTable {
                caption: &caption,
                columns: &columns,
                sorting: Sorting::default(),
                body: Body::Rows(rows),
            }),
            pager,
        },
    )
}

/// The person an action concerns, as a link to the patient.
fn person_link(visit: &Visit, action: &Action) -> Html {
    action.person.as_ref().map_or_else(
        || or_dash(&visit.locale, None),
        |person| {
            h(format!(
                "<a href=\"{}\"><code>{}</code></a>",
                t(format!("/patients/{person}")),
                t(person)
            ))
        },
    )
}

/// The disclosure of an action's other references.
fn details(visit: &Visit, action: &Action) -> Html {
    let code = |value: &str| h(format!("<code>{}</code>", t(value)));
    let mut rows: Vec<(String, Html)> = Vec::new();
    for (words, value) in [
        (Msg::DetailRecord, &action.record),
        (Msg::DetailCandidate, &action.candidate),
        (Msg::DetailMerge, &action.merge),
        (Msg::DetailDomain, &action.domain),
        (Msg::DetailPseudonymDomain, &action.pseudonym_domain),
        (Msg::DetailRequestId, &action.request_id),
    ] {
        if let Some(value) = value {
            rows.push((visit.tr(words), code(value)));
        }
    }
    rows.push((visit.tr(Msg::DetailAction), code(&action.id)));
    disclosure(visit.tr(Msg::ActionDetail), definitions(rows))
}
