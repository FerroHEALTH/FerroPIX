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

use ferrohealth_ui::html::Html;
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, label};
use ferrohealth_ui::view::compare::{Compare, CompareField, CompareRecord, Verdict, compare_pair};
use ferrohealth_ui::view::dialog::{ConfirmForm, Confirmation, danger_form_dialog};
use ferrohealth_ui::view::dock::{
    Decision, DecisionBar, DockField, DockForm, DockLine, DockLink, decision_bar,
};
use ferrohealth_ui::view::form::{Field, INPUT};
use ferrohealth_ui::view::identifier::{Domain, Masked, identifier_list};
use ferrohealth_ui::view::kbd::aria_keys;
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::score::{Contribution, Score, Threshold, score_breakdown};
use ferrohealth_ui::view::surface::{card, definitions, stack};
use ferrohealth_ui::view::time::timestamp;
use ferropix_operator_api::candidate::{
    Agreement, Candidate, CandidateRecord, FieldEvidence, Status,
};

use crate::app::QUEUE;
use crate::locale::messages::Msg;
use crate::markup::{h, hidden, t};
use crate::model::{DecisionOutcome, Probe, Reach};
use crate::pages::patient::{FIELDS, compare_record, value_of};
use crate::pages::queue::grade_words;
use crate::pages::{Needs, server_state};
use crate::visit::Visit;

/// The id of the decision bar; its hidden form is this and `-form`.
pub const BAR: &str = "decide";

/// The id of the dialog a link that merges asks in.
const MERGE_DIALOG: &str = "link-merges";

/// The pair page, the compare, the breakdown and the persons, and the
/// docked decision bar while the pair is undecided.
#[must_use]
pub fn pair(
    visit: &Visit,
    candidate: &Candidate,
    outcome: Option<&DecisionOutcome>,
) -> (Html, Option<Html>) {
    let changed = [&candidate.a, &candidate.b]
        .iter()
        .any(|record| record.head_version != record.version);
    let decided = candidate.status != Status::Open && candidate.status != Status::Deferred;
    let refused = match outcome {
        Some(DecisionOutcome::Refused(probe)) if !is_conflict(probe) => server_state(
            visit,
            Some(probe),
            Needs::of(Msg::WhatDecision),
            &format!("POST /operator/candidates/{}/decide", candidate.id),
        ),
        _ => Html::empty(),
    };
    let when = |shown: bool, part: Html| if shown { part } else { Html::empty() };
    let body = neighbours(visit, candidate).then(stack(Html::join([
        if decided {
            decided_notice(visit, candidate)
        } else {
            Html::empty()
        },
        when(
            changed,
            notice(
                Tone::Warn,
                Announce::Status,
                None,
                h(format!("<p>{}</p>", t(visit.tr(Msg::PairChanged)))),
            ),
        ),
        refused,
        card(visit.tr(Msg::PairCompare), records(visit, candidate)),
        card(visit.tr(Msg::PairBreakdown), breakdown(visit, candidate)),
        card(visit.tr(Msg::PairPersons), persons(visit, candidate)),
        if !decided && candidate.merges {
            merge_card(visit, candidate)
        } else {
            Html::empty()
        },
        if decided {
            Html::empty()
        } else {
            versions(candidate)
        },
    ])));
    let dock = (!decided).then(|| bar(visit, candidate, outcome));
    (body, dock)
}

/// Whether `probe` is the server's `409`: the pair changed under the page.
fn is_conflict(probe: &Probe) -> bool {
    probe.reach == Reach::Refused(http::StatusCode::CONFLICT)
}

/// Previous (`[`) and Next (`]`), the neighbouring pairs in the queue's
/// order, and the queue.
fn neighbours(visit: &Visit, candidate: &Candidate) -> Html {
    let link = |id: Option<&String>, words: Msg, key: &str, glyph: Glyph| {
        id.map_or_else(Html::empty, |id| {
            h(format!(
                "<a href=\"{}\" class=\"{}\" aria-keyshortcuts=\"{}\">{}</a>",
                t(format!("{QUEUE}/{id}")),
                Variant::Secondary.small(),
                t(aria_keys(key).unwrap_or_default()),
                label(Some(glyph), visit.tr(words)),
            ))
        })
    };
    h(format!(
        "<nav class=\"fui-actions\" aria-label=\"{}\">{}{}</nav>",
        t(visit.tr(Msg::PairNeighbours)),
        link(
            candidate.previous.as_ref(),
            Msg::PairPrevious,
            "[",
            Glyph::CHEVRON_LEFT
        ),
        link(
            candidate.next.as_ref(),
            Msg::PairNext,
            "]",
            Glyph::CHEVRON_RIGHT
        ),
    ))
}

/// Who decided the pair, when and how, for a pair someone already decided.
fn decided_notice(visit: &Visit, candidate: &Candidate) -> Html {
    let how = visit.tr(match candidate.status {
        Status::Linked => Msg::PairLinked,
        _ => Msg::PairNotAMatch,
    });
    let who = candidate.decided_by.clone().unwrap_or_default();
    notice(
        Tone::Info,
        Announce::Status,
        None,
        h(format!(
            "<p>{}</p>{}{}",
            t(visit.tr_with(
                Msg::PairDecided,
                &[("how", how.into()), ("who", who.into())]
            )),
            candidate
                .decided_at
                .as_ref()
                .map_or_else(Html::empty, |at| h(format!(
                    "<p class=\"fui-small fui-muted\">{}</p>",
                    timestamp(&visit.locale, at)
                ))),
            candidate
                .reason
                .as_ref()
                .map_or_else(Html::empty, |reason| h(format!(
                    "<p class=\"fui-small\">{}</p>",
                    t(reason)
                ))),
        )),
    )
}

/// The record's title: its domain, and its version on a quiet line.
fn record(visit: &Visit, key: &str, record: &CandidateRecord) -> CompareRecord {
    compare_record(
        visit,
        String::from(key),
        record.domain.name.clone(),
        Some(visit.tr_with(Msg::PairVersion, &[("version", record.version.into())])),
        &record.demographics,
    )
}

/// The two records side by side, the second marked against the first.
fn records(visit: &Visit, candidate: &Candidate) -> Html {
    let fields: Vec<CompareField> = FIELDS
        .iter()
        .map(|(key, words)| CompareField {
            key: (*key).to_owned(),
            label: visit.tr(*words),
        })
        .collect();
    let leading = record(visit, "a", &candidate.a);
    let others = vec![record(visit, "b", &candidate.b)];
    let (a, b) = (&candidate.a.demographics, &candidate.b.demographics);
    let verdict = |field: &str, _: usize| match (
        value_of(&visit.locale, a, field),
        value_of(&visit.locale, b, field),
    ) {
        (Some(lead), Some(theirs)) if lead.eq_ignore_ascii_case(&theirs) => Verdict::Agree,
        (Some(_), Some(_)) => Verdict::Differ,
        (Some(_), None) => Verdict::Absent,
        (None, _) => Verdict::Unmarked,
    };
    let caption = visit.tr(Msg::PairCompare);
    compare_pair(
        &visit.locale,
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
fn agreement(visit: &Visit, agreement: Agreement) -> String {
    visit.tr(match agreement {
        Agreement::Agree => Msg::AgreementAgree,
        Agreement::Partial => Msg::AgreementPartial,
        Agreement::Disagree => Msg::AgreementDisagree,
        Agreement::Absent => Msg::AgreementAbsent,
    })
}

/// One field's contribution, its words first and its comparator after.
fn contribution(visit: &Visit, field: &FieldEvidence) -> Contribution {
    Contribution {
        label: visit.tr_with(
            Msg::PairContribution,
            &[
                ("field", field.field.clone().into()),
                ("agreement", agreement(visit, field.agreement).into()),
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
fn breakdown(visit: &Visit, candidate: &Candidate) -> Html {
    let evidence = &candidate.evidence;
    let score = Score {
        total: evidence.total,
        value: evidence.score,
        grade: grade_words(visit, evidence.grade),
        contributions: evidence
            .fields
            .iter()
            .map(|field| contribution(visit, field))
            .collect(),
        thresholds: vec![
            Threshold {
                label: visit.tr(Msg::GradePossible),
                at: candidate.thresholds.possible,
            },
            Threshold {
                label: visit.tr(Msg::GradeProbable),
                at: candidate.thresholds.probable,
            },
            Threshold {
                label: visit.tr(Msg::GradeCertain),
                at: candidate.thresholds.certain,
            },
        ],
    };
    let key = evidence
        .blocking_key
        .clone()
        .unwrap_or_else(|| visit.tr(Msg::PairNoBlockingKey));
    let conflicts = if evidence.conflicts.is_empty() {
        Html::empty()
    } else {
        notice(
            Tone::Warn,
            Announce::Status,
            None,
            h(format!(
                "<p>{}</p>",
                t(visit.tr_with(
                    Msg::PairConflicts,
                    &[("conflicts", evidence.conflicts.join(", ").into())]
                ))
            )),
        )
    };
    h(format!(
        "<p class=\"fui-small\">{} <code>{}</code></p>{conflicts}{}",
        t(visit.tr(Msg::PairBlockingKey)),
        t(key),
        score_breakdown(&visit.locale, &score),
    ))
}

/// Each record's person, linked, with its identifiers masked.
fn persons(visit: &Visit, candidate: &Candidate) -> Html {
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
            h(format!(
                "<p><a href=\"{}\"><code>{}</code></a></p>{}",
                t(format!("/patients/{}", record.person)),
                t(visit.tr_with(Msg::PersonId, &[("person", record.person.clone().into())])),
                identifier_list(&visit.locale, &identifiers),
            )),
        )
    };
    definitions(vec![one(&candidate.a), one(&candidate.b)])
}

/// The hidden fields that carry the versions the page was drawn from with
/// the bar's form.
fn versions(candidate: &Candidate) -> Html {
    let form = format!("{BAR}-form");
    h(format!(
        "<input type=\"hidden\" form=\"{form}\" name=\"a_version\" value=\"{}\"><input type=\"hidden\" form=\"{form}\" name=\"b_version\" value=\"{}\">",
        candidate.a.version, candidate.b.version
    ))
}

/// The card that says a link merges two persons, with Link… (`l`), which
/// asks in the danger dialog naming the effect before it posts.
fn merge_card(visit: &Visit, candidate: &Candidate) -> Html {
    let action = format!("{QUEUE}/{}", candidate.id);
    let fields = Html::join([
        hidden("decision", "link"),
        hidden("a_version", &candidate.a.version.to_string()),
        hidden("b_version", &candidate.b.version.to_string()),
        Field::new("merge-reason", visit.tr(Msg::RevealReason)).view(h(format!(
            "<input id=\"merge-reason\" name=\"reason\" type=\"text\" class=\"{INPUT}\" required maxlength=\"500\" autocomplete=\"off\">"
        ))),
    ]);
    let confirm = h(format!(
        "<button type=\"submit\" class=\"{}\">{}</button>",
        Variant::Destructive.class(),
        label(Some(Glyph::LINK), visit.tr(Msg::PairLinkMerge))
    ));
    let dialog = danger_form_dialog(
        &visit.locale,
        Confirmation {
            id: MERGE_DIALOG.into(),
            title: visit.tr(Msg::PairMergeTitle).into(),
            message: visit
                .tr_with(
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
        visit.tr(Msg::PairMerges),
        h(format!(
            "<p>{}</p><button type=\"button\" class=\"{}\" command=\"show-modal\" commandfor=\"{MERGE_DIALOG}\" aria-haspopup=\"dialog\" aria-keyshortcuts=\"{}\">{}</button>{dialog}",
            t(visit.tr(Msg::PairMergesNote)),
            Variant::Primary.class(),
            t(aria_keys("l").unwrap_or_default()),
            label(Some(Glyph::LINK), visit.tr(Msg::PairLinkAsk)),
        )),
    )
}

/// The docked decision bar: the reason, the deferral's date, and Link (when
/// it merges nothing), Not a match and Defer.
fn bar(visit: &Visit, candidate: &Candidate, outcome: Option<&DecisionOutcome>) -> Html {
    let line = match outcome {
        Some(DecisionOutcome::Refused(probe)) if is_conflict(probe) => {
            let mut ways = Vec::new();
            if let Some(next) = &candidate.next {
                ways.push(DockLink {
                    href: format!("{QUEUE}/{next}"),
                    words: visit.tr(Msg::PairNext),
                    variant: Variant::Primary,
                });
            }
            ways.push(DockLink {
                href: format!("{QUEUE}/{}", candidate.id),
                words: visit.tr(Msg::PairReload),
                variant: Variant::Secondary,
            });
            Some(DockLine::Stale {
                said: visit.tr(Msg::PairStale),
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
            label: visit.tr(Msg::PairLink),
            variant: Variant::Primary,
            glyph: Some(Glyph::LINK),
            shortcut: Some(String::from("l")),
            ..Decision::default()
        });
    }
    decisions.push(Decision {
        value: String::from("not-a-match"),
        label: visit.tr(Msg::PairNotAMatchAction),
        glyph: Some(Glyph::X),
        shortcut: Some(String::from("n")),
        ..Decision::default()
    });
    decisions.push(Decision {
        value: String::from("defer"),
        label: visit.tr(Msg::Defer),
        glyph: Some(Glyph::CLOCK),
        shortcut: Some(String::from("d")),
        ..Decision::default()
    });
    decision_bar(DecisionBar {
        id: String::from(BAR),
        label: visit.tr(Msg::PairDecision),
        form: DockForm::Own(format!("{QUEUE}/{}", candidate.id)),
        fields: vec![
            DockField {
                id: String::from("decide-reason"),
                name: String::from("reason"),
                label: visit.tr(Msg::RevealReason),
                placeholder: visit.tr(Msg::PairReasonHint),
                shortcut: Some(String::from("r")),
                error: no_reason.then(|| visit.tr(Msg::PairReasonMissing)),
                focus: no_reason,
                ..DockField::default()
            },
            DockField {
                id: String::from("decide-until"),
                name: String::from("until"),
                label: visit.tr(Msg::DeferUntil),
                placeholder: visit.tr(Msg::DateHint),
                help: Some(visit.tr(Msg::PairUntilHelp)),
                ..DockField::default()
            },
        ],
        line,
        decisions,
    })
}
