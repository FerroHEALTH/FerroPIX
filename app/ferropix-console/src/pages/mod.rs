// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's pages, each drawn from the reviewed design in FerroUI's
//! `design/consoles/ferropix/`, and the state every page shares while the
//! FerroPIX server cannot answer it.
//!
//! [`server_state`] is that state: the refusal of a call that got no answer
//! or was refused, with Try again, the time of the call and its technical
//! detail, as the design's unreachable pages draw it; and, while the server
//! answers but serves no operator API yet, the notice that names the route
//! the page reads. A page never draws data the server did not send.

pub mod about;
pub mod dashboard;
pub mod not_found;
pub mod patient;
pub mod preferences;
pub mod pseudonyms;
pub mod registry;
pub mod screens;
pub mod search;
pub mod sign_in;
pub mod trail;

use ferrohealth_ui::i18n::Arg;
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, link_button};
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::refusal::{Issue, Refusal, Severity, refusal};
use ferrohealth_ui::view::surface::{card, cluster, definitions, disclosure, stack};
use ferrohealth_ui::view::time::timestamp;
use leptos::prelude::*;
use leptos_meta::Title;

use crate::app::current_path;
use crate::locale::{Msg, tr, tr_with};
use crate::model::{Failure, Probe, Reach};

/// The document title of the page `name`, which the root's title format
/// completes with the console's name.
#[must_use]
pub fn title(name: impl Into<String>) -> impl IntoView {
    view! { <Title text=name.into() /> }
}

/// The words of `failure`, as the technical detail names it.
fn failure_words(reach: &Reach) -> String {
    match reach {
        Reach::Up => String::new(),
        Reach::Refused(status) => tr_with(
            Msg::FailureStatus,
            &[("status", Arg::from(status.as_u16().to_string()))],
        ),
        Reach::Unreachable(Failure::Connect) => tr(Msg::FailureConnect),
        Reach::Unreachable(Failure::Timeout) => tr(Msg::FailureTimeout),
        Reach::Unreachable(Failure::Other) => tr(Msg::FailureOther),
    }
}

/// The refusal of the call `probe`, which `what` cannot be shown without.
fn probe_refusal(probe: &Probe, what: &str) -> Refusal {
    let what = Arg::from(what.to_owned());
    let (status, summary) = match &probe.reach {
        Reach::Refused(status) => (
            Some(status.as_u16()),
            tr_with(Msg::RefusedCall, &[("what", what)]),
        ),
        Reach::Unreachable(Failure::Connect) => {
            (None, tr_with(Msg::UnreachableConnect, &[("what", what)]))
        }
        Reach::Unreachable(Failure::Timeout) => {
            (None, tr_with(Msg::UnreachableTimeout, &[("what", what)]))
        }
        Reach::Unreachable(Failure::Other) | Reach::Up => {
            (None, tr_with(Msg::UnreachableOther, &[("what", what)]))
        }
    };
    let issues = probe
        .issues
        .iter()
        .map(|issue| {
            let severity = match issue.severity.as_str() {
                "fatal" => Severity::Fatal,
                "warning" => Severity::Warning,
                "information" => Severity::Information,
                _ => Severity::Error,
            };
            let shown = Issue::new(severity, issue.code.clone());
            match &issue.diagnostics {
                Some(text) => shown.text(text.clone()),
                None => shown,
            }
        })
        .collect();
    Refusal {
        status,
        summary,
        issues,
        ..Refusal::default()
    }
}

/// Try again, a link to the page itself, beside when the console last
/// called the server.
fn try_again(probe: &Probe) -> AnyView {
    cluster(view! {
        {link_button(current_path(), Variant::Secondary, Some(Glyph::REFRESH_CW), tr(Msg::TryAgain))}
        <span class="fui-small fui-muted">{tr(Msg::LastTried)} " " {timestamp(&probe.at)}</span>
    })
    .into_any()
}

/// The technical detail of the call `probe`, folded away.
fn technical_detail(probe: &Probe) -> AnyView {
    disclosure(
        tr(Msg::TechnicalDetail),
        definitions(vec![
            (
                tr(Msg::DetailFailure),
                failure_words(&probe.reach).into_any(),
            ),
            (
                tr(Msg::DetailRoute),
                view! { <code>{probe.route.clone()}</code> }.into_any(),
            ),
            (
                tr(Msg::DetailRequestId),
                view! { <code>{probe.request_id.clone()}</code> }.into_any(),
            ),
            (tr(Msg::DetailTime), timestamp(&probe.at).into_any()),
        ]),
    )
    .into_any()
}

/// What a page shows while the server cannot give it what it reads.
#[derive(Debug, Clone, Copy)]
pub struct Needs {
    /// What the page would show, as the refusal names it ("the queue").
    pub what: Msg,
    /// Whether the page says what the console keeps while the server is
    /// away, as the dashboard does.
    pub away: bool,
}

/// The state of a page that reads the operator API `route`.
///
/// It is drawn from the call `probe` to the server's liveness route: the
/// refusal with Try again and the technical detail when the server did not
/// answer or refused, and the notice naming the route when it answers.
#[must_use]
pub fn server_state(probe: Option<&Probe>, needs: Needs, route: &str) -> AnyView {
    let what = tr(needs.what);
    let Some(probe) = probe else {
        return ().into_any();
    };
    if probe.is_up() {
        return notice(
            Tone::Info,
            Announce::Status,
            None,
            view! {
                <p>{tr_with(Msg::NotServed, &[("what", Arg::from(what))])}</p>
                <p class="fui-small fui-muted">
                    {tr_with(Msg::NotServedRoute, &[("route", Arg::from(route.to_owned()))])}
                </p>
            },
        )
        .into_any();
    }
    let away = needs
        .away
        .then(|| card(tr(Msg::AwayTitle), view! { <p>{tr(Msg::AwayBody)}</p> }));
    stack(view! {
        {refusal(&probe_refusal(probe, &what))}
        {try_again(probe)}
        {away}
        {technical_detail(probe)}
    })
    .into_any()
}

/// The words of a steward action's or a source message's kind as the server
/// names it, or the name itself for a kind the console does not know.
#[must_use]
pub fn action_words(kind: &str) -> String {
    let words = match kind {
        "view" => Msg::ActionView,
        "export" => Msg::ActionExport,
        "reveal" => Msg::ActionReveal,
        "domain-create" => Msg::ActionDomainCreate,
        "domain-update" => Msg::ActionDomainUpdate,
        "feed" => Msg::ActionFeed,
        "remove" => Msg::ActionRemove,
        "resolve-duplicate" => Msg::ActionResolveDuplicate,
        "merge" => Msg::ActionMerge,
        "unmerge" => Msg::ActionUnmerge,
        "decide" => Msg::ActionDecide,
        "message" => Msg::ActionMessage,
        other => return other.to_owned(),
    };
    tr(words)
}

/// The words of an administrative gender code (the FHIR R4 `ValueSet`
/// `administrative-gender`), or the code itself for one outside it.
#[must_use]
pub fn gender_words(code: &str) -> String {
    let words = match code {
        "female" => Msg::GenderFemale,
        "male" => Msg::GenderMale,
        "other" => Msg::GenderOther,
        "unknown" => Msg::GenderUnknown,
        other => return other.to_owned(),
    };
    tr(words)
}
