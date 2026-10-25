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

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::table::view::{
    Body, Column, DataTable, Role, Sorting, cell, data_table, title_cell,
};
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::compare::{Compare, CompareField, CompareRecord, Verdict, compare};
use ferrohealth_ui::view::form::{Field, INPUT};
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::status::badge;
use ferrohealth_ui::view::surface::{card, definitions, or_none, stack};
use ferrohealth_ui::view::tabs::{Tab, binder};
use ferrohealth_ui::view::template::list::list_empty;
use ferrohealth_ui::view::time::timestamp;
use ferrohealth_ui::view::timeline::{Event, timeline};
use ferrohealth_ui::widget::countdown::{Countdown, countdown};
use ferrohealth_ui::widget::identifier::{Domain, Masked, identifier_static};
use ferropix_operator_api::patient::{Demographics, PersonStatus};
use ferropix_operator_api::person::{
    HistoryEvent, IdentifierHandle, LinkKind, Person, RecordState,
};
use ferropix_operator_api::reveal::Revealed;
use leptos::prelude::*;

use crate::document::Read;
use crate::locale::{Msg, tr, tr_with};
use crate::pages::registry::channel;
use crate::pages::{Needs, server_state};

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
pub fn patient(person: &Person, tab: &str, revealed: Option<&Read<Revealed>>) -> AnyView {
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
            Tab::new(*key, href, tr(*words))
        })
        .collect();
    let panel = match current {
        "records" => records(person),
        "history" => history(person),
        "pseudonyms" => pseudonyms(person),
        _ => identifiers(person),
    };
    stack(view! {
        {summary(person)}
        {revealed.map(reveal_answer)}
        {binder(tr(Msg::PatientTabs), tabs, current, panel)}
    })
    .into_any()
}

/// The leading demographics and the person's state.
fn summary(person: &Person) -> AnyView {
    let leading = person.leading.clone().unwrap_or_default();
    let status = match person.status {
        PersonStatus::Active => badge(Tone::Ok, tr(Msg::Active)).into_any(),
        PersonStatus::Merged => badge(Tone::Neutral, tr(Msg::Merged)).into_any(),
    };
    let merged_into = person.merged_into.clone().map(|survivor| {
        (
            tr(Msg::MergedInto),
            view! { <a href=format!("/patients/{survivor}")><code>{survivor.clone()}</code></a> }
                .into_any(),
        )
    });
    let mut rows = vec![
        (
            tr(Msg::FullName),
            full_name(&leading).map_or_else(|| or_none(String::new()), IntoAny::into_any),
        ),
        (
            tr(Msg::BirthDate),
            leading
                .birth_date
                .clone()
                .map_or_else(|| or_none(String::new()), IntoAny::into_any),
        ),
        (
            tr(Msg::Gender),
            leading.gender.as_deref().map_or_else(
                || or_none(String::new()),
                |code| crate::pages::gender_words(code).into_any(),
            ),
        ),
        (tr(Msg::ColState), status),
        (
            tr(Msg::ColCreated),
            timestamp(&person.created_at).into_any(),
        ),
    ];
    rows.extend(merged_into);
    card(tr(Msg::LeadingDemographics), definitions(rows)).into_any()
}

/// The answer of the reveal just posted: the value with its countdown, or
/// the refusal.
fn reveal_answer(revealed: &Read<Revealed>) -> AnyView {
    match revealed {
        Ok(revealed) => notice(
            Tone::Warn,
            Announce::Alert,
            None,
            view! {
                <p><strong>{tr(Msg::Revealed)}</strong></p>
                <p>
                    <code class="fui-wrap">{revealed.system.clone()}</code>
                    " "
                    <strong><code>{revealed.value.0.clone()}</code></strong>
                </p>
                {countdown(Countdown::new("reveal-countdown", revealed.seconds, revealed.seconds, &tr(Msg::RevealHidden)))}
                <p class="fui-small fui-muted">{tr_with(Msg::RevealRecorded, &[("action", revealed.action.clone().into())])}</p>
            },
        )
        .into_any(),
        Err(probe) => server_state(
            Some(probe),
            Needs {
                what: Msg::WhatReveal,
                away: false,
            },
            "POST /operator/persons/{id}/reveal",
        ),
    }
}

/// The reveal form of one identifier or pseudonym: the target, the reason,
/// and the submit.
fn reveal_form(person: &str, kind: &'static str, id: &str, row: usize) -> AnyView {
    let field_id = format!("reason-{row}");
    view! {
        <form method="post" action=format!("/patients/{person}/{REVEAL}") class="fui-form fui-inline-form">
            <input type="hidden" name="target" value=kind />
            <input type="hidden" name="id" value=id.to_owned() />
            {Field::new(field_id.clone(), tr(Msg::RevealReason)).view(view! {
                <input id=field_id.clone() name="reason" type="text" class=INPUT required maxlength="500" autocomplete="off" />
            })}
            <button type="submit" class=Variant::Secondary.class()>
                {label(Some(Glyph::EYE), tr(Msg::Reveal))}
            </button>
        </form>
    }
    .into_any()
}

/// The masked form of `masked` in `domain`.
fn masked(domain: &str, system: &str, masked: &str) -> AnyView {
    let tail = masked.trim_start_matches('•').trim().to_owned();
    identifier_static(
        &Masked {
            domain: Domain::new(domain).system(system),
            tail,
        },
        false,
    )
}

/// The words of a record's state.
fn record_state(state: RecordState) -> AnyView {
    match state {
        RecordState::Active => badge(Tone::Ok, tr(Msg::Active)).into_any(),
        RecordState::Subsumed => badge(Tone::Neutral, tr(Msg::Subsumed)).into_any(),
        RecordState::Removed => badge(Tone::Bad, tr(Msg::Removed)).into_any(),
    }
}

/// The words of a link's kind.
fn link_kind(kind: LinkKind) -> String {
    tr(match kind {
        LinkKind::Feed => Msg::LinkFeed,
        LinkKind::Deterministic => Msg::LinkDeterministic,
        LinkKind::Probabilistic => Msg::LinkProbabilistic,
        LinkKind::Steward => Msg::LinkSteward,
        LinkKind::Source => Msg::LinkSource,
        LinkKind::Merge => Msg::LinkMerge,
    })
}

/// The Identifiers tab: one row per identifier, masked, with its reveal.
fn identifiers(person: &Person) -> AnyView {
    if person.identifiers.is_empty() {
        return list_empty(tr(Msg::IdentifiersEmpty), None);
    }
    let columns = [
        Column::text("domain", tr(Msg::ColDomain)).role(Role::Title),
        Column::text("value", tr(Msg::ColValue)),
        Column::text("state", tr(Msg::ColState)),
        Column::text("link", tr(Msg::ColLink)).role(Role::Sub),
        Column::text("fed", tr(Msg::ColLastFed)).role(Role::Quiet),
        Column::text("reveal", tr(Msg::Reveal)).role(Role::Wide),
    ];
    let [_, value, state, link, fed, reveal] = &columns;
    let rows = person
        .identifiers
        .iter()
        .enumerate()
        .map(|(row, identifier)| {
            let (kind, id) = match &identifier.handle {
                IdentifierHandle::Record(id) => ("record", id.clone()),
                IdentifierHandle::Carried(id) => ("carried", id.clone()),
            };
            let linked = identifier.link.as_ref().map_or_else(
                || or_none(String::new()),
                |link| link_kind(link.kind).into_any(),
            );
            let fed_at = identifier.last_fed.as_ref().map_or_else(
                || or_none(String::new()),
                |at| timestamp(at).into_any(),
            );
            view! {
                <tr>
                    {title_cell(None, identifier.domain.name.clone(), None)}
                    {cell(value, masked(&identifier.domain.name, &identifier.domain.system, &identifier.masked.0))}
                    {cell(state, record_state(identifier.state))}
                    {cell(link, linked)}
                    {cell(fed, fed_at)}
                    {cell(reveal, reveal_form(&person.id, kind, &id, row))}
                </tr>
            }
        })
        .collect_view()
        .into_any();
    data_table(DataTable {
        caption: &tr(Msg::TabIdentifiers),
        columns: &columns,
        sorting: Sorting::default(),
        body: Body::Rows(rows),
    })
    .into_any()
}

/// The compared fields of a record: name, birth date, gender, address and
/// telecom.
const FIELDS: [(&str, Msg); 5] = [
    ("name", Msg::FullName),
    ("birth", Msg::BirthDate),
    ("gender", Msg::Gender),
    ("address", Msg::Address),
    ("telecom", Msg::Telecom),
];

/// The value of the field `key` of `demographics`, as text.
fn value_of(demographics: &Demographics, key: &str) -> Option<String> {
    match key {
        "name" => full_name(demographics),
        "birth" => demographics.birth_date.clone(),
        "gender" => demographics
            .gender
            .as_deref()
            .map(crate::pages::gender_words),
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
fn compare_record(
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
            .map(|(field, _)| {
                value_of(demographics, field)
                    .map(|value| ViewFn::from(move || value.clone().into_any()))
            })
            .collect(),
    }
}

/// The Records tab: the source records side by side, each field marked
/// against the leading demographics.
fn records(person: &Person) -> AnyView {
    if person.records.is_empty() {
        return list_empty(tr(Msg::RecordsEmpty), None);
    }
    let leading_demographics = person.leading.clone().unwrap_or_default();
    let fields: Vec<CompareField> = FIELDS
        .iter()
        .map(|(key, words)| CompareField {
            key: (*key).to_owned(),
            label: tr(*words),
        })
        .collect();
    let leading = compare_record(
        String::from("leading"),
        tr(Msg::LeadingDemographics),
        None,
        &leading_demographics,
    );
    let others: Vec<CompareRecord> = person
        .records
        .iter()
        .map(|record| {
            compare_record(
                record.id.clone(),
                record.domain.name.clone(),
                Some(tr_with(
                    Msg::RecordLine,
                    &[
                        ("version", record.version.into()),
                        ("channel", channel(record.channel).into()),
                    ],
                )),
                &record.demographics,
            )
        })
        .collect();
    let demographics: Vec<Demographics> = person
        .records
        .iter()
        .map(|record| record.demographics.clone())
        .collect();
    let verdict = move |field: &str, index: usize| {
        let lead = value_of(&leading_demographics, field);
        let theirs = demographics.get(index).and_then(|d| value_of(d, field));
        match (lead, theirs) {
            (Some(lead), Some(theirs)) if lead.eq_ignore_ascii_case(&theirs) => Verdict::Agree,
            (Some(_), Some(_)) => Verdict::Differ,
            (Some(_), None) => Verdict::Absent,
            (None, _) => Verdict::Unmarked,
        }
    };
    let caption = tr(Msg::TabRecords);
    compare(Compare {
        id: "records",
        caption: &caption,
        fields: &fields,
        leading: &leading,
        others: &others,
        verdict: &verdict,
        differ_first: false,
    })
}

/// The History tab: the person's versions, links, merges and actions on a
/// timeline.
fn history(person: &Person) -> AnyView {
    if person.history.is_empty() {
        return list_empty(tr(Msg::HistoryEmpty), None);
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
                    tr(Msg::EventVersion),
                    Tone::Neutral,
                    tr_with(
                        Msg::RecordLine,
                        &[
                            ("version", (*version).into()),
                            ("channel", channel(*arrived).into()),
                        ],
                    ),
                ),
                HistoryEvent::Linked { kind, .. } => {
                    (tr(Msg::EventLinked), Tone::Ok, link_kind(*kind))
                }
                HistoryEvent::Unlinked { .. } => {
                    (tr(Msg::EventUnlinked), Tone::Warn, String::new())
                }
                HistoryEvent::Merged { survivor, .. } => (
                    tr(Msg::EventMerged),
                    Tone::Info,
                    tr_with(Msg::MergedIntoLine, &[("person", survivor.clone().into())]),
                ),
                HistoryEvent::Unmerged { merged, .. } => (
                    tr(Msg::EventUnmerged),
                    Tone::Info,
                    tr_with(Msg::RestoredLine, &[("person", merged.clone().into())]),
                ),
                HistoryEvent::Action { kind } => {
                    (tr(Msg::EventAction), Tone::Neutral, kind.clone())
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
    let truncated = person.history_truncated.then(|| {
        view! { <p class="fui-small fui-muted">{tr(Msg::HistoryTruncated)}</p> }
    });
    view! {
        {timeline(tr(Msg::TabHistory), &events)}
        {truncated}
    }
    .into_any()
}

/// The Pseudonyms tab: each pseudonymisation domain and whether the person
/// has a pseudonym in it, masked, with its reveal.
fn pseudonyms(person: &Person) -> AnyView {
    if person.pseudonyms.is_empty() {
        return list_empty(tr(Msg::PseudonymsEmpty), None);
    }
    let columns = [
        Column::text("name", tr(Msg::ColName)).role(Role::Title),
        Column::text("namespace", tr(Msg::ColNamespace)).role(Role::Sub),
        Column::text("value", tr(Msg::ColValue)),
        Column::text("reveal", tr(Msg::Reveal)).role(Role::Wide),
    ];
    let [_, namespace, value, reveal] = &columns;
    let offset = person.identifiers.len();
    let rows = person
        .pseudonyms
        .iter()
        .enumerate()
        .map(|(row, pseudonym)| {
            let shown = pseudonym.masked.as_ref().map_or_else(
                || tr(Msg::NoPseudonym).into_any(),
                |held| masked(&pseudonym.name, &pseudonym.namespace, &held.0),
            );
            let form = pseudonym.masked.as_ref().map(|_| {
                reveal_form(&person.id, "pseudonym", &pseudonym.domain, offset.saturating_add(row))
            });
            view! {
                <tr>
                    {title_cell(None, pseudonym.name.clone(), None)}
                    {cell(namespace, view! { <code class="fui-wrap">{pseudonym.namespace.clone()}</code> })}
                    {cell(value, shown)}
                    {cell(reveal, form)}
                </tr>
            }
        })
        .collect_view()
        .into_any();
    data_table(DataTable {
        caption: &tr(Msg::TabPseudonyms),
        columns: &columns,
        sorting: Sorting::default(),
        body: Body::Rows(rows),
    })
    .into_any()
}
