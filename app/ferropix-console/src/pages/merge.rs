// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The merge and the unmerge of a patient, each with the full preview of
//! its effect from `GET /operator/merges/preview` (`docs/architecture.md`
//! §12.3 and §12.6).
//!
//! A merge names the other patient, then shows the two patients with the
//! survivor chosen (the older by default), the identifiers that move, the
//! pseudonyms that resolve to the survivor, the subscribers told and the
//! candidates closed; the reason and FerroUI's danger dialog, which names
//! the count, come before anything is posted. The unmerge shows the
//! latest merge into the patient with its reason and the preview of its
//! revert, or the merge that has to be reverted first. Both post the
//! versions the preview read, so one made on a stale preview comes back as
//! `409` and is drawn in place. No specification governs the screens: our
//! own design; PMIR 1.6.0 §2:3.93.4.1.3 admits the local unmerge policy.

use ferrohealth_ui::html::Html;
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::dialog::{ConfirmForm, Confirmation, danger_form_dialog};
use ferrohealth_ui::view::form::{Field, INPUT};
use ferrohealth_ui::view::identifier::{Domain, Masked, identifier_list};
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::surface::{card, definitions, stack};
use ferrohealth_ui::view::time::timestamp;
use ferropix_operator_api::candidate::PersonIdentifier;
use ferropix_operator_api::merge::{Party, Preview};

use crate::locale::messages::Msg;
use crate::markup::{h, hidden, t};
use crate::model::{Probe, Read};
use crate::pages::{Needs, or_dash, server_state};
use crate::visit::Visit;

/// The hidden field that names which of the page's forms was posted.
pub const FORM: &str = "form";

/// The value of [`FORM`] for a preview.
pub const PREVIEW: &str = "preview";

/// The value of [`FORM`] for the merge itself.
pub const MERGE: &str = "merge";

/// The value of [`FORM`] for the revert.
pub const REVERT: &str = "revert";

/// The id of the merge's danger dialog.
const MERGE_DIALOG: &str = "merge-confirm";

/// The id of the revert's danger dialog.
const REVERT_DIALOG: &str = "revert-confirm";

/// The masked identifiers of `identifiers`, as FerroUI draws them.
fn masked(identifiers: &[PersonIdentifier]) -> Vec<Masked> {
    identifiers
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
        .collect()
}

/// A party: its name, or its id when it states none, linked to its page,
/// and when it was made.
fn party(visit: &Visit, party: &Party) -> Html {
    let name = party
        .name
        .clone()
        .unwrap_or_else(|| visit.tr_with(Msg::PersonId, &[("person", party.id.clone().into())]));
    h(format!(
        "<a href=\"{}\">{}</a><p class=\"fui-small fui-muted\">{} {}</p>",
        t(format!("/patients/{}", party.id)),
        t(name),
        t(visit.tr(Msg::MergeMade)),
        timestamp(&visit.locale, &party.created_at),
    ))
}

/// The effect a preview names: what moves, what resolves elsewhere, who is
/// told and what closes.
fn effect(visit: &Visit, preview: &Preview) -> Html {
    let locale = &visit.locale;
    let identifiers = if preview.identifiers.is_empty() {
        or_dash(locale, None)
    } else {
        identifier_list(locale, &masked(&preview.identifiers))
    };
    let pseudonyms = if preview.pseudonyms.is_empty() {
        or_dash(locale, None)
    } else {
        let shown: Vec<Masked> = preview
            .pseudonyms
            .iter()
            .map(|pseudonym| Masked {
                domain: Domain::new(pseudonym.name.clone()).system(pseudonym.namespace.clone()),
                tail: pseudonym
                    .masked
                    .0
                    .trim_start_matches('\u{2022}')
                    .trim()
                    .to_owned(),
            })
            .collect();
        identifier_list(locale, &shown)
    };
    definitions(vec![
        (visit.tr(Msg::MergeIdentifiers), identifiers),
        (
            visit.tr(Msg::MergePseudonyms),
            h(format!(
                "<ul class=\"fui-identifier-list\">{pseudonyms}</ul>"
            )),
        ),
        (
            visit.tr(Msg::MergeSubscriptions),
            t(visit.tr_with(Msg::Count, &[("count", preview.subscriptions.into())])),
        ),
        (
            visit.tr(Msg::MergeCandidates),
            t(visit.tr_with(
                Msg::Count,
                &[(
                    "count",
                    u64::try_from(preview.candidates.len())
                        .unwrap_or(u64::MAX)
                        .into(),
                )],
            )),
        ),
    ])
}

/// A refused merge or revert, drawn above the page: a `409` says the
/// preview went stale, any other refusal is the call's state.
fn refused_notice(visit: &Visit, refused: Option<&Probe>, route: &str) -> Html {
    refused.map_or_else(Html::empty, |probe| {
        if probe.reach == crate::model::Reach::Refused(http::StatusCode::CONFLICT) {
            notice(
                Tone::Warn,
                Announce::Alert,
                None,
                h(format!("<p>{}</p>", t(visit.tr(Msg::MergeStale)))),
            )
        } else {
            server_state(visit, Some(probe), Needs::of(Msg::WhatMergeWrite), route)
        }
    })
}

/// The required reason of a danger dialog.
fn reason_field(visit: &Visit, id: &'static str) -> Html {
    Field::new(id, visit.tr(Msg::RevealReason)).view(h(format!(
        "<input id=\"{id}\" name=\"reason\" type=\"text\" class=\"{INPUT}\" required maxlength=\"500\" autocomplete=\"off\">"
    )))
}

/// A danger button that opens the dialog `dialog`, showing `glyph` and
/// `words`.
fn asks(dialog: &str, glyph: Glyph, words: &str) -> Html {
    h(format!(
        "<button type=\"button\" class=\"{}\" command=\"show-modal\" commandfor=\"{}\" aria-haspopup=\"dialog\">{}</button>",
        Variant::Destructive.class(),
        t(dialog),
        label(Some(glyph), words)
    ))
}

/// The submit of a danger dialog, showing `glyph` and `words`.
fn confirms(glyph: Glyph, words: &str) -> Html {
    h(format!(
        "<button type=\"submit\" class=\"{}\">{}</button>",
        Variant::Destructive.class(),
        label(Some(glyph), words)
    ))
}

/// The merge page of `person`: the form that names the other patient, and
/// once previewed, the two patients, the choice of survivor, the effect and
/// the dialog that merges.
#[must_use]
pub fn merge(
    visit: &Visit,
    person: &str,
    preview: Option<&Read<Preview>>,
    refused: Option<&Probe>,
) -> Html {
    let action = format!("/patients/{person}/merge");
    let other = preview
        .and_then(|read| read.as_ref().ok())
        .map(|preview| {
            if preview.survivor.id == person {
                preview.merged.id.clone()
            } else {
                preview.survivor.id.clone()
            }
        })
        .unwrap_or_default();
    let other_field = Field::new("merge-other", visit.tr(Msg::MergeOtherField)).view(h(format!(
        "<input id=\"merge-other\" name=\"other\" type=\"text\" class=\"{INPUT}\" required autocomplete=\"off\" spellcheck=\"false\" value=\"{}\">",
        t(&other)
    )));
    let pick = card(
        visit.tr(Msg::MergeOther),
        h(format!(
            "<form method=\"post\" action=\"{}\" class=\"fui-form\">{}<p class=\"fui-help\">{}</p><div class=\"fui-form-grid\">{other_field}</div><div class=\"fui-form-foot\"><button type=\"submit\" class=\"{}\">{}</button></div></form>",
            t(&action),
            hidden(FORM, PREVIEW),
            t(visit.tr(Msg::MergeOtherNote)),
            Variant::Secondary.class(),
            label(Some(Glyph::EYE), visit.tr(Msg::MergePreview)),
        )),
    );
    let shown = preview.map_or_else(Html::empty, |read| match read {
        Ok(preview) => previewed(visit, person, &action, preview),
        Err(probe) => server_state(
            visit,
            Some(probe),
            Needs::of(Msg::WhatMergePreview),
            "GET /operator/merges/preview",
        ),
    });
    stack(Html::join([
        refused_notice(visit, refused, "POST /operator/merges"),
        pick,
        shown,
    ]))
}

/// A merge's preview: the survivor choice, the effect, and the dialog that
/// merges with the reason and both versions.
fn previewed(visit: &Visit, person: &str, action: &str, preview: &Preview) -> Html {
    let survivor = &preview.survivor;
    let merged = &preview.merged;
    let other = if survivor.id == person {
        &merged.id
    } else {
        &survivor.id
    };
    let swap = h(format!(
        "<form method=\"post\" action=\"{}\" class=\"fui-inline-form\">{}{}{}<button type=\"submit\" class=\"{}\">{}</button></form>",
        t(action),
        hidden(FORM, PREVIEW),
        hidden("other", other),
        hidden("survivor", &merged.id),
        Variant::Secondary.small(),
        label(Some(Glyph::ARROW_LEFT_RIGHT), visit.tr(Msg::MergeSwap)),
    ));
    let moved = u64::try_from(preview.identifiers.len()).unwrap_or(u64::MAX);
    let fields = Html::join([
        hidden(FORM, MERGE),
        hidden("survivor", &survivor.id),
        hidden("survivor_version", &survivor.version),
        hidden("merged", &merged.id),
        hidden("merged_version", &merged.version),
        reason_field(visit, "merge-reason"),
    ]);
    let dialog = danger_form_dialog(
        &visit.locale,
        Confirmation {
            id: MERGE_DIALOG.into(),
            title: visit.tr(Msg::MergeConfirmTitle).into(),
            message: visit
                .tr_with(Msg::MergeConfirmMessage, &[("count", moved.into())])
                .into(),
        },
        ConfirmForm {
            action: action.to_owned(),
            fields,
        },
        confirms(Glyph::GIT_COMPARE, &visit.tr(Msg::Merge)),
    );
    let parties = definitions(vec![
        (visit.tr(Msg::MergeSurvivor), party(visit, survivor)),
        (visit.tr(Msg::MergeMerged), party(visit, merged)),
    ]);
    card(
        visit.tr(Msg::MergeEffect),
        Html::join([
            parties,
            swap,
            effect(visit, preview),
            h(format!(
                "<p class=\"fui-help\">{}</p>",
                t(visit.tr(Msg::MergeReversible))
            )),
            asks(MERGE_DIALOG, Glyph::GIT_COMPARE, &visit.tr(Msg::MergeAsk)),
            dialog,
        ]),
    )
}

/// The unmerge page of `person`.
///
/// It draws the latest merge into it, its reason and the preview of its
/// revert with the dialog that reverts it; the merge to revert first when
/// this one is not the latest; or the note that there is nothing to revert.
#[must_use]
pub fn unmerge(
    visit: &Visit,
    person: &str,
    merge: Option<&String>,
    preview: Option<&Read<Preview>>,
    refused: Option<&Probe>,
) -> Html {
    let action = format!("/patients/{person}/unmerge");
    let body = match (merge, preview) {
        (Some(merge), Some(Ok(preview))) => reverted(visit, &action, merge, preview),
        (_, Some(Err(probe))) => server_state(
            visit,
            Some(probe),
            Needs::of(Msg::WhatMergePreview),
            "GET /operator/merges/preview",
        ),
        _ => notice(
            Tone::Info,
            Announce::Status,
            None,
            h(format!("<p>{}</p>", t(visit.tr(Msg::UnmergeNone)))),
        ),
    };
    stack(refused_notice(visit, refused, "POST /operator/merges/{id}/revert").then(body))
}

/// A revert's preview: the merge with its reason, what moves back, and the
/// dialog that reverts it, or the merge to revert first.
fn reverted(visit: &Visit, action: &str, merge: &str, preview: &Preview) -> Html {
    let first = preview
        .first_revert
        .as_ref()
        .map_or_else(Html::empty, |first| {
            notice(
                Tone::Warn,
                Announce::Status,
                None,
                h(format!(
                    "<p>{}</p>",
                    t(visit.tr_with(Msg::UnmergeFirst, &[("merge", first.clone().into())]))
                )),
            )
        });
    let facts = definitions(vec![
        (
            visit.tr(Msg::MergeSurvivor),
            party(visit, &preview.survivor),
        ),
        (
            visit.tr(Msg::UnmergeRestored),
            party(visit, &preview.merged),
        ),
        (
            visit.tr(Msg::ColReason),
            or_dash(&visit.locale, preview.reason.clone()),
        ),
    ]);
    let ask = if preview.first_revert.is_none() {
        let moved = u64::try_from(preview.identifiers.len()).unwrap_or(u64::MAX);
        let fields = Html::join([
            hidden(FORM, REVERT),
            hidden("merge", merge),
            hidden("survivor_version", &preview.survivor.version),
            reason_field(visit, "revert-reason"),
        ]);
        let dialog = danger_form_dialog(
            &visit.locale,
            Confirmation {
                id: REVERT_DIALOG.into(),
                title: visit.tr(Msg::UnmergeConfirmTitle).into(),
                message: visit
                    .tr_with(Msg::UnmergeConfirmMessage, &[("count", moved.into())])
                    .into(),
            },
            ConfirmForm {
                action: action.to_owned(),
                fields,
            },
            confirms(Glyph::UNDO, &visit.tr(Msg::Unmerge)),
        );
        asks(REVERT_DIALOG, Glyph::UNDO, &visit.tr(Msg::UnmergeAsk)).then(dialog)
    } else {
        Html::empty()
    };
    card(
        visit.tr(Msg::UnmergeEffect),
        Html::join([first, facts, effect(visit, preview), ask]),
    )
}

/// The newest merge into `person` that no revert in its history undid.
#[must_use]
pub fn latest_merge(person: &ferropix_operator_api::person::Person) -> Option<String> {
    use ferropix_operator_api::person::HistoryEvent;
    let reverted: Vec<&String> = person
        .history
        .iter()
        .filter_map(|entry| match &entry.event {
            HistoryEvent::Unmerged { merge, .. } => Some(merge),
            _ => None,
        })
        .collect();
    person.history.iter().find_map(|entry| match &entry.event {
        HistoryEvent::Merged {
            merge, survivor, ..
        } if *survivor == person.id && !reverted.contains(&merge) => Some(merge.clone()),
        _ => None,
    })
}
