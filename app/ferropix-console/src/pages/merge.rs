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

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::dialog::{ConfirmForm, Confirmation, danger_form_dialog};
use ferrohealth_ui::view::form::{Field, INPUT};
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::surface::{card, definitions, or_none, stack};
use ferrohealth_ui::view::time::timestamp;
use ferrohealth_ui::widget::identifier::{Domain, Masked, identifier_list};
use ferropix_operator_api::candidate::PersonIdentifier;
use ferropix_operator_api::merge::{Party, Preview};
use leptos::prelude::*;

use crate::locale::messages::Msg;
use crate::locale::{tr, tr_with};
use crate::model::{Probe, Read};
use crate::pages::{Needs, server_state};

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
fn party(party: &Party) -> AnyView {
    let name = party
        .name
        .clone()
        .unwrap_or_else(|| tr_with(Msg::PersonId, &[("person", party.id.clone().into())]));
    view! {
        <a href=format!("/patients/{}", party.id)>{name}</a>
        <p class="fui-small fui-muted">{tr(Msg::MergeMade)} " " {timestamp(&party.created_at)}</p>
    }
    .into_any()
}

/// The effect a preview names: what moves, what resolves elsewhere, who is
/// told and what closes.
fn effect(preview: &Preview) -> AnyView {
    let identifiers = if preview.identifiers.is_empty() {
        or_none(String::new())
    } else {
        identifier_list(&masked(&preview.identifiers))
    };
    let pseudonyms = if preview.pseudonyms.is_empty() {
        or_none(String::new())
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
        identifier_list(&shown)
    };
    definitions(vec![
        (tr(Msg::MergeIdentifiers), identifiers),
        (
            tr(Msg::MergePseudonyms),
            view! { <ul class="fui-identifier-list">{pseudonyms}</ul> }.into_any(),
        ),
        (
            tr(Msg::MergeSubscriptions),
            tr_with(Msg::Count, &[("count", preview.subscriptions.into())]).into_any(),
        ),
        (
            tr(Msg::MergeCandidates),
            tr_with(
                Msg::Count,
                &[(
                    "count",
                    u64::try_from(preview.candidates.len())
                        .unwrap_or(u64::MAX)
                        .into(),
                )],
            )
            .into_any(),
        ),
    ])
    .into_any()
}

/// A refused merge or revert, drawn above the page: a `409` says the
/// preview went stale, any other refusal is the call's state.
fn refused_notice(refused: Option<&Probe>, route: &str) -> Option<AnyView> {
    refused.map(|probe| {
        if probe.reach == crate::model::Reach::Refused(http::StatusCode::CONFLICT) {
            notice(
                Tone::Warn,
                Announce::Alert,
                None,
                view! { <p>{tr(Msg::MergeStale)}</p> },
            )
            .into_any()
        } else {
            server_state(
                Some(probe),
                Needs {
                    what: Msg::WhatMergeWrite,
                    away: false,
                },
                route,
            )
        }
    })
}

/// The required reason of a danger dialog.
fn reason_field(id: &'static str) -> AnyView {
    Field::new(id, tr(Msg::RevealReason)).view(view! {
        <input id=id name="reason" type="text" class=INPUT required maxlength="500" autocomplete="off" />
    })
}

/// The merge page of `person`: the form that names the other patient, and
/// once previewed, the two patients, the choice of survivor, the effect and
/// the dialog that merges.
#[must_use]
pub fn merge(person: &str, preview: Option<&Read<Preview>>, refused: Option<&Probe>) -> AnyView {
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
    let pick = card(
        tr(Msg::MergeOther),
        view! {
            <form method="post" action=action.clone() class="fui-form">
                <input type="hidden" name=FORM value=PREVIEW />
                <p class="fui-help">{tr(Msg::MergeOtherNote)}</p>
                <div class="fui-form-grid">
                    {Field::new("merge-other", tr(Msg::MergeOtherField)).view(view! {
                        <input id="merge-other" name="other" type="text" class=INPUT required autocomplete="off" spellcheck="false" value=other.clone() />
                    })}
                </div>
                <div class="fui-form-foot">
                    <button type="submit" class=Variant::Secondary.class()>{label(Some(Glyph::EYE), tr(Msg::MergePreview))}</button>
                </div>
            </form>
        },
    );
    let shown = preview.map(|read| match read {
        Ok(preview) => previewed(person, &action, preview),
        Err(probe) => server_state(
            Some(probe),
            Needs {
                what: Msg::WhatMergePreview,
                away: false,
            },
            "GET /operator/merges/preview",
        ),
    });
    stack(view! {
        {refused_notice(refused, "POST /operator/merges")}
        {pick}
        {shown}
    })
    .into_any()
}

/// A merge's preview: the survivor choice, the effect, and the dialog that
/// merges with the reason and both versions.
fn previewed(person: &str, action: &str, preview: &Preview) -> AnyView {
    let survivor = &preview.survivor;
    let merged = &preview.merged;
    let swap = view! {
        <form method="post" action=action.to_owned() class="fui-inline-form">
            <input type="hidden" name=FORM value=PREVIEW />
            <input type="hidden" name="other" value=if survivor.id == person { merged.id.clone() } else { survivor.id.clone() } />
            <input type="hidden" name="survivor" value=merged.id.clone() />
            <button type="submit" class=Variant::Secondary.small()>{label(Some(Glyph::ARROW_LEFT_RIGHT), tr(Msg::MergeSwap))}</button>
        </form>
    };
    let moved = u64::try_from(preview.identifiers.len()).unwrap_or(u64::MAX);
    let fields = view! {
        <input type="hidden" name=FORM value=MERGE />
        <input type="hidden" name="survivor" value=survivor.id.clone() />
        <input type="hidden" name="survivor_version" value=survivor.version.clone() />
        <input type="hidden" name="merged" value=merged.id.clone() />
        <input type="hidden" name="merged_version" value=merged.version.clone() />
        {reason_field("merge-reason")}
    }
    .into_any();
    let dialog = danger_form_dialog(
        Confirmation {
            id: MERGE_DIALOG.into(),
            title: tr(Msg::MergeConfirmTitle).into(),
            message: tr_with(Msg::MergeConfirmMessage, &[("count", moved.into())]).into(),
        },
        ConfirmForm {
            action: action.to_owned(),
            fields,
        },
        view! { <button type="submit" class=Variant::Destructive.class()>{label(Some(Glyph::GIT_COMPARE), tr(Msg::Merge))}</button> },
    );
    let parties = definitions(vec![
        (tr(Msg::MergeSurvivor), party(survivor)),
        (tr(Msg::MergeMerged), party(merged)),
    ]);
    card(
        tr(Msg::MergeEffect),
        view! {
            {parties}
            {swap}
            {effect(preview)}
            <p class="fui-help">{tr(Msg::MergeReversible)}</p>
            <button type="button" class=Variant::Destructive.class() command="show-modal" commandfor=MERGE_DIALOG aria-haspopup="dialog">
                {label(Some(Glyph::GIT_COMPARE), tr(Msg::MergeAsk))}
            </button>
            {dialog}
        },
    )
    .into_any()
}

/// The unmerge page of `person`.
///
/// It draws the latest merge into it, its reason and the preview of its
/// revert with the dialog that reverts it; the merge to revert first when
/// this one is not the latest; or the note that there is nothing to revert.
#[must_use]
pub fn unmerge(
    person: &str,
    merge: Option<&String>,
    preview: Option<&Read<Preview>>,
    refused: Option<&Probe>,
) -> AnyView {
    let action = format!("/patients/{person}/unmerge");
    let body = match (merge, preview) {
        (Some(merge), Some(Ok(preview))) => reverted(&action, merge, preview),
        (_, Some(Err(probe))) => server_state(
            Some(probe),
            Needs {
                what: Msg::WhatMergePreview,
                away: false,
            },
            "GET /operator/merges/preview",
        ),
        _ => notice(
            Tone::Info,
            Announce::Status,
            None,
            view! { <p>{tr(Msg::UnmergeNone)}</p> },
        )
        .into_any(),
    };
    stack(view! {
        {refused_notice(refused, "POST /operator/merges/{id}/revert")}
        {body}
    })
    .into_any()
}

/// A revert's preview: the merge with its reason, what moves back, and the
/// dialog that reverts it, or the merge to revert first.
fn reverted(action: &str, merge: &str, preview: &Preview) -> AnyView {
    let first = preview.first_revert.as_ref().map(|first| {
        notice(
            Tone::Warn,
            Announce::Status,
            None,
            view! { <p>{tr_with(Msg::UnmergeFirst, &[("merge", first.clone().into())])}</p> },
        )
    });
    let facts = definitions(vec![
        (tr(Msg::MergeSurvivor), party(&preview.survivor)),
        (tr(Msg::UnmergeRestored), party(&preview.merged)),
        (
            tr(Msg::ColReason),
            preview
                .reason
                .clone()
                .map_or_else(|| or_none(String::new()), IntoAny::into_any),
        ),
    ]);
    let ask = preview.first_revert.is_none().then(|| {
        let moved = u64::try_from(preview.identifiers.len()).unwrap_or(u64::MAX);
        let fields = view! {
            <input type="hidden" name=FORM value=REVERT />
            <input type="hidden" name="merge" value=merge.to_owned() />
            <input type="hidden" name="survivor_version" value=preview.survivor.version.clone() />
            {reason_field("revert-reason")}
        }
        .into_any();
        let dialog = danger_form_dialog(
            Confirmation {
                id: REVERT_DIALOG.into(),
                title: tr(Msg::UnmergeConfirmTitle).into(),
                message: tr_with(Msg::UnmergeConfirmMessage, &[("count", moved.into())]).into(),
            },
            ConfirmForm {
                action: action.to_owned(),
                fields,
            },
            view! { <button type="submit" class=Variant::Destructive.class()>{label(Some(Glyph::UNDO), tr(Msg::Unmerge))}</button> },
        );
        view! {
            <button type="button" class=Variant::Destructive.class() command="show-modal" commandfor=REVERT_DIALOG aria-haspopup="dialog">
                {label(Some(Glyph::UNDO), tr(Msg::UnmergeAsk))}
            </button>
            {dialog}
        }
    });
    card(
        tr(Msg::UnmergeEffect),
        view! {
            {first}
            {facts}
            {effect(preview)}
            {ask}
        },
    )
    .into_any()
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
