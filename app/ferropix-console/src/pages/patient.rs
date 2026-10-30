// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The patient record at `/patients/{person}` (design 05, 05.10): the
//! leading demographics, then four tabs, Identifiers, Records, History and
//! Pseudonyms, from `GET /operator/persons/{id}`.
//!
//! Every identifier and pseudonym is masked. A reveal is a posted form with
//! the steward's reason, a server round trip the server records as a
//! privacy disclosure, and the page drawn again shows the value with a
//! countdown for the seconds the server allows. The tab a steward reads is
//! a query word, never an identifier. The records stand side by side in
//! FerroUI's record compare, each field marked against the leading
//! demographics in words and tone. No specification governs the console:
//! our own design (decision D9: the person id is the one identifier an
//! address carries).

use ferrohealth_ui::html::Html;
use ferrohealth_ui::i18n::Locale;
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::table::view::{
    Body, Column, DataTable, Role, Sorting, cell, data_table, title_cell,
};
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::compare::{Compare, CompareField, CompareRecord, Verdict, compare};
use ferrohealth_ui::view::countdown::{Countdown, countdown};
use ferrohealth_ui::view::form::{Field, INPUT};
use ferrohealth_ui::view::identifier::{Domain, Masked, identifier_static};
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::status::badge;
use ferrohealth_ui::view::surface::{card, definitions, stack};
use ferrohealth_ui::view::tabs::{Tab, binder};
use ferrohealth_ui::view::template::list::list_empty;
use ferrohealth_ui::view::time::timestamp;
use ferrohealth_ui::view::timeline::{Event, timeline};
use ferropix_operator_api::patient::{Demographics, PersonStatus};
use ferropix_operator_api::person::{
    HistoryEvent, IdentifierHandle, LinkKind, Person, RecordState,
};
use ferropix_operator_api::reveal::Revealed;

use crate::locale::messages::Msg;
use crate::markup::{h, hidden, t};
use crate::model::Read;
use crate::pages::registry::channel;
use crate::pages::{Needs, or_dash, server_state};
use crate::visit::Visit;

/// The tabs of the patient record, each its key and its words.
pub const TABS: [(&str, Msg); 4] = [
    ("identifiers", Msg::TabIdentifiers),
    ("records", Msg::TabRecords),
    ("history", Msg::TabHistory),
    ("pseudonyms", Msg::TabPseudonyms),
];

/// The route a reveal posts to, under the person.
pub const REVEAL: &str = "reveal";

/// The full name of `demographics`, the first name it states.
#[must_use]
pub fn full_name(demographics: &Demographics) -> Option<String> {
    let name = demographics.names.first()?;
    let mut parts: Vec<String> = name.given.clone();
    parts.extend(name.family.clone());
    let joined = parts.join(" ");
    (!joined.is_empty()).then_some(joined)
}

/// The patient record, on the tab `tab`, with a reveal just answered.
#[must_use]
pub fn patient(
    visit: &Visit,
    person: &Person,
    tab: &str,
    revealed: Option<&Read<Revealed>>,
) -> Html {
    let current = TABS
        .iter()
        .find(|(key, _)| *key == tab)
        .map_or("identifiers", |(key, _)| key);
    let base = format!("/patients/{}", person.id);
    let tabs = TABS
        .iter()
        .map(|(key, words)| {
            let href = if *key == "identifiers" {
                base.clone()
            } else {
                format!("{base}?tab={key}")
            };
            Tab::new(*key, href, visit.tr(*words))
        })
        .collect();
    let panel = match current {
        "records" => records(visit, person),
        "history" => history(visit, person),
        "pseudonyms" => pseudonyms(visit, person),
        _ => identifiers(visit, person),
    };
    let unmerge = crate::pages::merge::latest_merge(person).map_or_else(Html::empty, |_| {
        h(format!(
            "<a href=\"{}\" class=\"{}\">{}</a>",
            t(format!("{base}/unmerge")),
            Variant::Secondary.class(),
            label(Some(Glyph::UNDO), visit.tr(Msg::PatientUnmerge))
        ))
    });
    let actions = h(format!(
        "<div class=\"fui-actions\"><a href=\"{}\" class=\"{}\">{}</a>{unmerge}</div>",
        t(format!("{base}/merge")),
        Variant::Secondary.class(),
        label(Some(Glyph::GIT_COMPARE), visit.tr(Msg::PatientMergeInto))
    ));
    stack(Html::join([
        actions,
        summary(visit, person),
        revealed.map_or_else(Html::empty, |revealed| reveal_answer(visit, revealed)),
        binder(
            &visit.locale,
            visit.tr(Msg::PatientTabs),
            tabs,
            current,
            panel,
        ),
    ]))
}

/// The leading demographics and the person's state.
fn summary(visit: &Visit, person: &Person) -> Html {
    let locale = &visit.locale;
    let leading = person.leading.clone().unwrap_or_default();
    let status = match person.status {
        PersonStatus::Active => badge(Tone::Ok, visit.tr(Msg::StateActive)),
        PersonStatus::Merged => badge(Tone::Neutral, visit.tr(Msg::Merged)),
    };
    let mut rows = vec![
        (
            visit.tr(Msg::FullName),
            or_dash(locale, full_name(&leading)),
        ),
        (
            visit.tr(Msg::BirthDate),
            or_dash(locale, leading.birth_date.clone()),
        ),
        (
            visit.tr(Msg::Gender),
            or_dash(
                locale,
                leading
                    .gender
                    .as_deref()
                    .map(|code| crate::pages::gender_words(locale, code)),
            ),
        ),
        (visit.tr(Msg::ColState), status),
        (
            visit.tr(Msg::ColCreated),
            timestamp(locale, &person.created_at),
        ),
    ];
    if let Some(survivor) = &person.merged_into {
        rows.push((
            visit.tr(Msg::MergedInto),
            h(format!(
                "<a href=\"{}\"><code>{}</code></a>",
                t(format!("/patients/{survivor}")),
                t(survivor)
            )),
        ));
    }
    card(visit.tr(Msg::LeadingDemographics), definitions(rows))
}

/// The answer of the reveal just posted: the value with its countdown, or
/// the refusal.
fn reveal_answer(visit: &Visit, revealed: &Read<Revealed>) -> Html {
    match revealed {
        Ok(revealed) => notice(
            Tone::Warn,
            Announce::Alert,
            None,
            h(format!(
                "<p><strong>{}</strong></p><p><code class=\"fui-wrap\">{}</code> <strong><code>{}</code></strong></p>{}<p class=\"fui-small fui-muted\">{}</p>",
                t(visit.tr(Msg::Revealed)),
                t(&revealed.system),
                t(&revealed.value.0),
                countdown(Countdown::new(
                    &visit.locale,
                    "reveal-countdown",
                    revealed.seconds,
                    revealed.seconds,
                    &visit.tr(Msg::RevealHidden),
                )),
                t(visit.tr_with(
                    Msg::RevealRecorded,
                    &[("action", revealed.action.clone().into())]
                )),
            )),
        ),
        Err(probe) => server_state(
            visit,
            Some(probe),
            Needs::of(Msg::WhatReveal),
            "POST /operator/persons/{id}/reveal",
        ),
    }
}

/// The reveal form of one identifier or pseudonym: the target, the reason,
/// and the submit.
fn reveal_form(visit: &Visit, person: &str, kind: &'static str, id: &str, row: usize) -> Html {
    let field_id = format!("reason-{row}");
    let reason = Field::new(field_id.clone(), visit.tr(Msg::RevealReason)).view(h(format!(
        "<input id=\"{}\" name=\"reason\" type=\"text\" class=\"{INPUT}\" required maxlength=\"500\" autocomplete=\"off\">",
        t(&field_id)
    )));
    h(format!(
        "<form method=\"post\" action=\"{}\" class=\"fui-form fui-inline-form\">{}{}{reason}<button type=\"submit\" class=\"{}\">{}</button></form>",
        t(format!("/patients/{person}/{REVEAL}")),
        hidden("target", kind),
        hidden("id", id),
        Variant::Secondary.class(),
        label(Some(Glyph::EYE), visit.tr(Msg::Reveal)),
    ))
}

/// The masked form of `masked` in `domain`.
fn masked(locale: &Locale, domain: &str, system: &str, masked: &str) -> Html {
    let tail = masked.trim_start_matches('•').trim().to_owned();
    identifier_static(
        locale,
        &Masked {
            domain: Domain::new(domain).system(system),
            tail,
        },
        false,
    )
}

/// The words of a record's state.
fn record_state(visit: &Visit, state: RecordState) -> Html {
    match state {
        RecordState::Active => badge(Tone::Ok, visit.tr(Msg::StateActive)),
        RecordState::Subsumed => badge(Tone::Neutral, visit.tr(Msg::Subsumed)),
        RecordState::Removed => badge(Tone::Bad, visit.tr(Msg::Removed)),
    }
}

/// The words of a link's kind.
fn link_kind(visit: &Visit, kind: LinkKind) -> String {
    visit.tr(match kind {
        LinkKind::Feed => Msg::LinkFeed,
        LinkKind::Deterministic => Msg::LinkDeterministic,
        LinkKind::Probabilistic => Msg::LinkProbabilistic,
        LinkKind::Steward => Msg::LinkSteward,
        LinkKind::Source => Msg::LinkSource,
        LinkKind::Merge => Msg::LinkMerge,
    })
}

/// The Identifiers tab: one row per identifier, masked, with its reveal.
fn identifiers(visit: &Visit, person: &Person) -> Html {
    let locale = &visit.locale;
    if person.identifiers.is_empty() {
        return list_empty(visit.tr(Msg::IdentifiersEmpty), None);
    }
    let columns = [
        Column::text("domain", visit.tr(Msg::ColDomain)).role(Role::Title),
        Column::text("value", visit.tr(Msg::ColValue)),
        Column::text("state", visit.tr(Msg::ColState)),
        Column::text("link", visit.tr(Msg::ColLink)).role(Role::Sub),
        Column::text("fed", visit.tr(Msg::ColLastFed)).role(Role::Quiet),
        Column::text("reveal", visit.tr(Msg::Reveal)).role(Role::Wide),
    ];
    let [_, value, state, link, fed, reveal] = &columns;
    let rows: Html = person
        .identifiers
        .iter()
        .enumerate()
        .map(|(row, identifier)| {
            let (kind, id) = match &identifier.handle {
                IdentifierHandle::Record(id) => ("record", id.as_str()),
                IdentifierHandle::Carried(id) => ("carried", id.as_str()),
            };
            let linked = or_dash(
                locale,
                identifier
                    .link
                    .as_ref()
                    .map(|link| link_kind(visit, link.kind)),
            );
            let fed_at = identifier
                .last_fed
                .as_ref()
                .map_or_else(|| or_dash(locale, None), |at| timestamp(locale, at));
            h(format!(
                "<tr>{}{}{}{}{}{}</tr>",
                title_cell(None, t(&identifier.domain.name), None),
                cell(
                    value,
                    masked(
                        locale,
                        &identifier.domain.name,
                        &identifier.domain.system,
                        &identifier.masked.0
                    )
                ),
                cell(state, record_state(visit, identifier.state)),
                cell(link, linked),
                cell(fed, fed_at),
                cell(reveal, reveal_form(visit, &person.id, kind, id, row)),
            ))
        })
        .collect();
    let caption = visit.tr(Msg::TabIdentifiers);
    data_table(DataTable {
        caption: &caption,
        columns: &columns,
        sorting: Sorting::default(),
        body: Body::Rows(rows),
    })
}

/// The compared fields of a record: name, birth date, gender, address and
/// telecom.
pub(crate) const FIELDS: [(&str, Msg); 5] = [
    ("name", Msg::FullName),
    ("birth", Msg::BirthDate),
    ("gender", Msg::Gender),
    ("address", Msg::Address),
    ("telecom", Msg::Telecom),
];

/// The value of the field `key` of `demographics`, as text in `locale`.
pub(crate) fn value_of(locale: &Locale, demographics: &Demographics, key: &str) -> Option<String> {
    match key {
        "name" => full_name(demographics),
        "birth" => demographics.birth_date.clone(),
        "gender" => demographics
            .gender
            .as_deref()
            .map(|code| crate::pages::gender_words(locale, code)),
        "address" => demographics.addresses.first().map(|address| {
            let mut parts = address.lines.clone();
            parts.extend(address.postal_code.clone());
            parts.extend(address.city.clone());
            parts.extend(address.country.clone());
            parts.join(", ")
        }),
        _ => demographics
            .telecom
            .first()
            .map(|telecom| telecom.value.clone()),
    }
    .filter(|value| !value.is_empty())
}

/// One record of the compare.
pub(crate) fn compare_record(
    visit: &Visit,
    key: String,
    title: String,
    sub: Option<String>,
    demographics: &Demographics,
) -> CompareRecord {
    CompareRecord {
        key,
        title,
        sub,
        values: FIELDS
            .iter()
            .map(|(field, _)| value_of(&visit.locale, demographics, field).map(t))
            .collect(),
    }
}

/// The Records tab: the source records side by side, each field marked
/// against the leading demographics.
fn records(visit: &Visit, person: &Person) -> Html {
    let locale = &visit.locale;
    if person.records.is_empty() {
        return list_empty(visit.tr(Msg::RecordsEmpty), None);
    }
    let leading_demographics = person.leading.clone().unwrap_or_default();
    let fields: Vec<CompareField> = FIELDS
        .iter()
        .map(|(key, words)| CompareField {
            key: (*key).to_owned(),
            label: visit.tr(*words),
        })
        .collect();
    let leading = compare_record(
        visit,
        String::from("leading"),
        visit.tr(Msg::LeadingDemographics),
        None,
        &leading_demographics,
    );
    let others: Vec<CompareRecord> = person
        .records
        .iter()
        .map(|record| {
            compare_record(
                visit,
                record.id.clone(),
                record.domain.name.clone(),
                Some(visit.tr_with(
                    Msg::RecordLine,
                    &[
                        ("version", record.version.into()),
                        ("channel", channel(visit, record.channel).into()),
                    ],
                )),
                &record.demographics,
            )
        })
        .collect();
    let verdict = |field: &str, index: usize| {
        let lead = value_of(locale, &leading_demographics, field);
        let theirs = person
            .records
            .get(index)
            .and_then(|record| value_of(locale, &record.demographics, field));
        match (lead, theirs) {
            (Some(lead), Some(theirs)) if lead.eq_ignore_ascii_case(&theirs) => Verdict::Agree,
            (Some(_), Some(_)) => Verdict::Differ,
            (Some(_), None) => Verdict::Absent,
            (None, _) => Verdict::Unmarked,
        }
    };
    let caption = visit.tr(Msg::TabRecords);
    compare(
        locale,
        Compare {
            id: "records",
            caption: &caption,
            fields: &fields,
            leading: &leading,
            others: &others,
            verdict: &verdict,
            differ_first: false,
        },
    )
}

/// The History tab: the person's versions, links, merges and actions on a
/// timeline.
fn history(visit: &Visit, person: &Person) -> Html {
    if person.history.is_empty() {
        return list_empty(visit.tr(Msg::HistoryEmpty), None);
    }
    let events: Vec<Event> = person
        .history
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let (kind, tone, title) = match &entry.event {
                HistoryEvent::Version {
                    version,
                    channel: arrived,
                    ..
                } => (
                    visit.tr(Msg::EventVersion),
                    Tone::Neutral,
                    visit.tr_with(
                        Msg::RecordLine,
                        &[
                            ("version", (*version).into()),
                            ("channel", channel(visit, *arrived).into()),
                        ],
                    ),
                ),
                HistoryEvent::Linked { kind, .. } => (
                    visit.tr(Msg::EventLinked),
                    Tone::Ok,
                    link_kind(visit, *kind),
                ),
                HistoryEvent::Unlinked { .. } => {
                    (visit.tr(Msg::EventUnlinked), Tone::Warn, String::new())
                }
                HistoryEvent::Merged { survivor, .. } => (
                    visit.tr(Msg::EventMerged),
                    Tone::Info,
                    visit.tr_with(Msg::MergedIntoLine, &[("person", survivor.clone().into())]),
                ),
                HistoryEvent::Unmerged { merged, .. } => (
                    visit.tr(Msg::EventUnmerged),
                    Tone::Info,
                    visit.tr_with(Msg::RestoredLine, &[("person", merged.clone().into())]),
                ),
                HistoryEvent::Action { kind } => {
                    (visit.tr(Msg::EventAction), Tone::Neutral, kind.clone())
                }
            };
            Event {
                key: format!("event-{index}"),
                at: entry.at.clone(),
                kind,
                code: None,
                tone,
                actor: entry.actor.clone(),
                title,
                identifiers: Vec::new(),
                reason: entry.reason.clone(),
                body: None,
            }
        })
        .collect();
    let truncated = if person.history_truncated {
        h(format!(
            "<p class=\"fui-small fui-muted\">{}</p>",
            t(visit.tr(Msg::HistoryTruncated))
        ))
    } else {
        Html::empty()
    };
    timeline(&visit.locale, visit.tr(Msg::TabHistory), &events).then(truncated)
}

/// The Pseudonyms tab: each pseudonymisation domain and whether the person
/// has a pseudonym in it, masked, with its reveal, and the link to the one
/// place a pseudonym is resolved.
fn pseudonyms(visit: &Visit, person: &Person) -> Html {
    let locale = &visit.locale;
    if person.pseudonyms.is_empty() {
        return list_empty(visit.tr(Msg::PseudonymsEmpty), None);
    }
    let columns = [
        Column::text("name", visit.tr(Msg::ColName)).role(Role::Title),
        Column::text("namespace", visit.tr(Msg::ColNamespace)).role(Role::Sub),
        Column::text("value", visit.tr(Msg::ColValue)),
        Column::text("reveal", visit.tr(Msg::Reveal)).role(Role::Wide),
    ];
    let [_, namespace, value, reveal] = &columns;
    let offset = person.identifiers.len();
    let rows: Html = person
        .pseudonyms
        .iter()
        .enumerate()
        .map(|(row, pseudonym)| {
            let shown = pseudonym.masked.as_ref().map_or_else(
                || t(visit.tr(Msg::NoPseudonym)),
                |held| masked(locale, &pseudonym.name, &pseudonym.namespace, &held.0),
            );
            let form = pseudonym.masked.as_ref().map_or_else(Html::empty, |_| {
                reveal_form(
                    visit,
                    &person.id,
                    "pseudonym",
                    &pseudonym.domain,
                    offset.saturating_add(row),
                )
            });
            h(format!(
                "<tr>{}{}{}{}</tr>",
                title_cell(None, t(&pseudonym.name), None),
                cell(
                    namespace,
                    h(format!(
                        "<code class=\"fui-wrap\">{}</code>",
                        t(&pseudonym.namespace)
                    ))
                ),
                cell(value, shown),
                cell(reveal, form),
            ))
        })
        .collect();
    let caption = visit.tr(Msg::TabPseudonyms);
    let table = data_table(DataTable {
        caption: &caption,
        columns: &columns,
        sorting: Sorting::default(),
        body: Body::Rows(rows),
    });
    table.then(h(format!(
        "<p class=\"fui-small\"><a href=\"{}\">{}</a></p>",
        crate::app::PSEUDONYM_DOMAINS,
        t(visit.tr(Msg::ResolveTitle))
    )))
}
