// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! One pair to compare and decide, drawn from `GET
//! /operator/candidates/{id}` (`docs/architecture.md` §5.3, §12.3, §12.6).
//!
//! The two records side by side in FerroUI's record compare, the differing
//! rows first; the score breakdown with each field's weight, the total and
//! the thresholds on one scale; the blocking key and any hard conflict;
//! each record's person with its identifiers, masked; and FerroUI's docked
//! decision bar with Link (`l`), Not a match (`n`), Defer (`d`), the reason
//! (`r`) and the date a deferral comes back. A link that merges two persons
//! asks first in FerroUI's danger dialog, which names the effect. A
//! decision posts the record versions the page was drawn from, so one
//! made on a stale page comes back as `409`, drawn in the bar with the ways
//! on. Previous (`[`) and Next (`]`) lead to the neighbouring pairs. No
//! specification governs the screen: our own design.

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::compare::{Compare, CompareField, CompareRecord, Verdict, compare_pair};
use ferrohealth_ui::view::dialog::{ConfirmForm, Confirmation, danger_form_dialog};
use ferrohealth_ui::view::dock::{
    Decision, DecisionBar, DockField, DockForm, DockLine, DockLink, decision_bar,
};
use ferrohealth_ui::view::form::{Field, INPUT};
use ferrohealth_ui::view::kbd::aria_keys;
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::score::{Contribution, Score, Threshold, score_breakdown};
use ferrohealth_ui::view::surface::{card, definitions, stack};
use ferrohealth_ui::view::time::timestamp;
use ferrohealth_ui::widget::identifier::{Domain, Masked, identifier_list};
use ferropix_operator_api::candidate::{
    Agreement, Candidate, CandidateRecord, FieldEvidence, Status,
};
use leptos::prelude::*;

use crate::app::QUEUE;
use crate::locale::messages::Msg;
use crate::locale::{tr, tr_with};
use crate::model::{DecisionOutcome, Probe, Reach};
use crate::pages::patient::{FIELDS, compare_record, value_of};
use crate::pages::queue::grade_words;
use crate::pages::{Needs, server_state};

/// The id of the decision bar; its hidden form is this and `-form`.
pub const BAR: &str = "decide";

/// The id of the dialog a link that merges asks in.
const MERGE_DIALOG: &str = "link-merges";

/// The pair page: the compare, the breakdown, the persons and the bar.
#[must_use]
pub fn pair(candidate: &Candidate, outcome: Option<&DecisionOutcome>) -> AnyView {
    let changed = [&candidate.a, &candidate.b]
        .iter()
        .any(|record| record.head_version != record.version);
    let decided = candidate.status != Status::Open && candidate.status != Status::Deferred;
    let refused = outcome.and_then(|outcome| match outcome {
        DecisionOutcome::Refused(probe) if !is_conflict(probe) => Some(server_state(
            Some(probe),
            Needs {
                what: Msg::WhatDecision,
                away: false,
            },
            &format!("POST /operator/candidates/{}/decide", candidate.id),
        )),
        _ => None,
    });
    view! {
        {neighbours(candidate)}
        {stack(view! {
            {decided.then(|| decided_notice(candidate))}
            {changed.then(|| notice(Tone::Warn, Announce::Status, None, view! { <p>{tr(Msg::PairChanged)}</p> }).into_any())}
            {refused}
            {card(tr(Msg::PairCompare), records(candidate))}
            {card(tr(Msg::PairBreakdown), breakdown(candidate))}
            {card(tr(Msg::PairPersons), persons(candidate))}
            {(!decided && candidate.merges).then(|| merge_card(candidate))}
            {(!decided).then(|| versions(candidate))}
        })}
        {(!decided).then(|| bar(candidate, outcome))}
    }
    .into_any()
}

/// Whether `probe` is the server's `409`: the pair changed under the page.
fn is_conflict(probe: &Probe) -> bool {
    probe.reach == Reach::Refused(http::StatusCode::CONFLICT)
}

/// Previous (`[`) and Next (`]`), the neighbouring pairs in the queue's
/// order, and the queue.
fn neighbours(candidate: &Candidate) -> AnyView {
    let link = |id: &Option<String>, words: Msg, key: &str, glyph: Glyph| {
        id.as_ref().map(|id| {
            view! {
                <a href=format!("{QUEUE}/{id}") class=Variant::Secondary.small() aria-keyshortcuts=aria_keys(key)>
                    {label(Some(glyph), tr(words))}
                </a>
            }
        })
    };
    view! {
        <nav class="fui-actions" aria-label=tr(Msg::PairNeighbours)>
            {link(&candidate.previous, Msg::PairPrevious, "[", Glyph::CHEVRON_LEFT)}
            {link(&candidate.next, Msg::PairNext, "]", Glyph::CHEVRON_RIGHT)}
        </nav>
    }
    .into_any()
}

/// Who decided the pair, when and how, for a pair someone already decided.
fn decided_notice(candidate: &Candidate) -> AnyView {
    let how = tr(match candidate.status {
        Status::Linked => Msg::PairLinked,
        _ => Msg::PairNotAMatch,
    });
    let who = candidate.decided_by.clone().unwrap_or_default();
    notice(
        Tone::Info,
        Announce::Status,
        None,
        view! {
            <p>{tr_with(Msg::PairDecided, &[("how", how.into()), ("who", who.into())])}</p>
            {candidate.decided_at.as_ref().map(|at| view! { <p class="fui-small fui-muted">{timestamp(at)}</p> })}
            {candidate.reason.clone().map(|reason| view! { <p class="fui-small">{reason}</p> })}
        },
    )
    .into_any()
}

/// The record's title: its domain, and its version on a quiet line.
fn record(key: &str, record: &CandidateRecord) -> CompareRecord {
    compare_record(
        String::from(key),
        record.domain.name.clone(),
        Some(tr_with(
            Msg::PairVersion,
            &[("version", record.version.into())],
        )),
        &record.demographics,
    )
}

/// The two records side by side, the second marked against the first.
fn records(candidate: &Candidate) -> AnyView {
    let fields: Vec<CompareField> = FIELDS
        .iter()
        .map(|(key, words)| CompareField {
            key: (*key).to_owned(),
            label: tr(*words),
        })
        .collect();
    let leading = record("a", &candidate.a);
    let others = vec![record("b", &candidate.b)];
    let (a, b) = (
        candidate.a.demographics.clone(),
        candidate.b.demographics.clone(),
    );
    let verdict = move |field: &str, _: usize| match (value_of(&a, field), value_of(&b, field)) {
        (Some(lead), Some(theirs)) if lead.eq_ignore_ascii_case(&theirs) => Verdict::Agree,
        (Some(_), Some(_)) => Verdict::Differ,
        (Some(_), None) => Verdict::Absent,
        (None, _) => Verdict::Unmarked,
    };
    let caption = tr(Msg::PairCompare);
    compare_pair(
        Compare {
            id: "pair",
            caption: &caption,
            fields: &fields,
            leading: &leading,
            others: &others,
            verdict: &verdict,
            differ_first: true,
        },
        0,
    )
}

/// The words of a field's agreement.
fn agreement(agreement: Agreement) -> String {
    tr(match agreement {
        Agreement::Agree => Msg::AgreementAgree,
        Agreement::Partial => Msg::AgreementPartial,
        Agreement::Disagree => Msg::AgreementDisagree,
        Agreement::Absent => Msg::AgreementAbsent,
    })
}

/// One field's contribution, its words first and its comparator after.
fn contribution(field: &FieldEvidence) -> Contribution {
    Contribution {
        label: tr_with(
            Msg::PairContribution,
            &[
                ("field", field.field.clone().into()),
                ("agreement", agreement(field.agreement).into()),
            ],
        ),
        reading: Some(match field.reading {
            Some(reading) => format!("{} {reading:.2}", field.comparator),
            None => field.comparator.clone(),
        }),
        weight: field.weight,
    }
}

/// The score breakdown, the blocking key and the hard conflicts.
fn breakdown(candidate: &Candidate) -> AnyView {
    let evidence = &candidate.evidence;
    let score = Score {
        total: evidence.total,
        value: evidence.score,
        grade: grade_words(evidence.grade),
        contributions: evidence.fields.iter().map(contribution).collect(),
        thresholds: vec![
            Threshold {
                label: tr(Msg::GradePossible),
                at: candidate.thresholds.possible,
            },
            Threshold {
                label: tr(Msg::GradeProbable),
                at: candidate.thresholds.probable,
            },
            Threshold {
                label: tr(Msg::GradeCertain),
                at: candidate.thresholds.certain,
            },
        ],
    };
    let key = evidence
        .blocking_key
        .clone()
        .unwrap_or_else(|| tr(Msg::PairNoBlockingKey));
    let conflicts = (!evidence.conflicts.is_empty()).then(|| {
        notice(
            Tone::Warn,
            Announce::Status,
            None,
            view! { <p>{tr_with(Msg::PairConflicts, &[("conflicts", evidence.conflicts.join(", ").into())])}</p> },
        )
    });
    view! {
        <p class="fui-small">{tr(Msg::PairBlockingKey)} " " <code>{key}</code></p>
        {conflicts}
        {score_breakdown(&score)}
    }
    .into_any()
}

/// Each record's person, linked, with its identifiers masked.
fn persons(candidate: &Candidate) -> AnyView {
    let one = |record: &CandidateRecord| {
        let identifiers: Vec<Masked> = record
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
        (
            record.domain.name.clone(),
            view! {
                <p>
                    <a href=format!("/patients/{}", record.person)>
                        <code>{tr_with(Msg::PersonId, &[("person", record.person.clone().into())])}</code>
                    </a>
                </p>
                {identifier_list(&identifiers)}
            }
            .into_any(),
        )
    };
    definitions(vec![one(&candidate.a), one(&candidate.b)]).into_any()
}

/// The hidden fields that carry the versions the page was drawn from with
/// the bar's form.
fn versions(candidate: &Candidate) -> AnyView {
    let form = format!("{BAR}-form");
    view! {
        <input type="hidden" form=form.clone() name="a_version" value=candidate.a.version.to_string() />
        <input type="hidden" form=form name="b_version" value=candidate.b.version.to_string() />
    }
    .into_any()
}

/// The card that says a link merges two persons, with Link… (`l`), which
/// asks in the danger dialog naming the effect before it posts.
fn merge_card(candidate: &Candidate) -> AnyView {
    let action = format!("{QUEUE}/{}", candidate.id);
    let fields = view! {
        <input type="hidden" name="decision" value="link" />
        <input type="hidden" name="a_version" value=candidate.a.version.to_string() />
        <input type="hidden" name="b_version" value=candidate.b.version.to_string() />
        {Field::new("merge-reason", tr(Msg::RevealReason)).view(view! {
            <input id="merge-reason" name="reason" type="text" class=INPUT required maxlength="500" autocomplete="off" />
        })}
    }
    .into_any();
    let confirm = view! {
        <button type="submit" class=Variant::Destructive.class()>{label(Some(Glyph::LINK), tr(Msg::PairLinkMerge))}</button>
    };
    let dialog = danger_form_dialog(
        Confirmation {
            id: MERGE_DIALOG.into(),
            title: tr(Msg::PairMergeTitle).into(),
            message: tr_with(
                Msg::PairMergeMessage,
                &[
                    ("a", candidate.a.person.clone().into()),
                    ("b", candidate.b.person.clone().into()),
                ],
            )
            .into(),
        },
        ConfirmForm { action, fields },
        confirm,
    );
    card(
        tr(Msg::PairMerges),
        view! {
            <p>{tr(Msg::PairMergesNote)}</p>
            <button type="button" class=Variant::Primary.class() command="show-modal" commandfor=MERGE_DIALOG aria-haspopup="dialog" aria-keyshortcuts=aria_keys("l")>
                {label(Some(Glyph::LINK), tr(Msg::PairLinkAsk))}
            </button>
            {dialog}
        },
    )
    .into_any()
}

/// The docked decision bar: the reason, the deferral's date, and Link (when
/// it merges nothing), Not a match and Defer.
fn bar(candidate: &Candidate, outcome: Option<&DecisionOutcome>) -> AnyView {
    let line = match outcome {
        Some(DecisionOutcome::Refused(probe)) if is_conflict(probe) => {
            let mut ways = Vec::new();
            if let Some(next) = &candidate.next {
                ways.push(DockLink {
                    href: format!("{QUEUE}/{next}"),
                    words: tr(Msg::PairNext),
                    variant: Variant::Primary,
                });
            }
            ways.push(DockLink {
                href: format!("{QUEUE}/{}", candidate.id),
                words: tr(Msg::PairReload),
                variant: Variant::Secondary,
            });
            Some(DockLine::Stale {
                said: tr(Msg::PairStale),
                ways,
            })
        }
        _ => None,
    };
    let no_reason = matches!(outcome, Some(DecisionOutcome::NoReason));
    let mut decisions = Vec::new();
    if !candidate.merges {
        decisions.push(Decision {
            value: String::from("link"),
            label: tr(Msg::PairLink),
            variant: Variant::Primary,
            glyph: Some(Glyph::LINK),
            shortcut: Some(String::from("l")),
            ..Decision::default()
        });
    }
    decisions.push(Decision {
        value: String::from("not-a-match"),
        label: tr(Msg::PairNotAMatchAction),
        glyph: Some(Glyph::X),
        shortcut: Some(String::from("n")),
        ..Decision::default()
    });
    decisions.push(Decision {
        value: String::from("defer"),
        label: tr(Msg::Defer),
        glyph: Some(Glyph::CLOCK),
        shortcut: Some(String::from("d")),
        ..Decision::default()
    });
    decision_bar(DecisionBar {
        id: String::from(BAR),
        label: tr(Msg::PairDecision),
        form: DockForm::Own(format!("{QUEUE}/{}", candidate.id)),
        fields: vec![
            DockField {
                id: String::from("decide-reason"),
                name: String::from("reason"),
                label: tr(Msg::RevealReason),
                placeholder: tr(Msg::PairReasonHint),
                shortcut: Some(String::from("r")),
                error: no_reason.then(|| tr(Msg::PairReasonMissing)),
                focus: no_reason,
                ..DockField::default()
            },
            DockField {
                id: String::from("decide-until"),
                name: String::from("until"),
                label: tr(Msg::DeferUntil),
                placeholder: String::from("YYYY-MM-DD"),
                help: Some(tr(Msg::PairUntilHelp)),
                ..DockField::default()
            },
        ],
        line,
        decisions,
    })
}
