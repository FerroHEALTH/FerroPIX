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
pub mod merge;
pub mod not_found;
pub mod pair;
pub mod patient;
pub mod preferences;
pub mod pseudonyms;
pub mod queue;
pub mod registry;
pub mod screens;
pub mod search;
pub mod sign_in;
pub mod trail;

use ferrohealth_ui::html::Html;
use ferrohealth_ui::i18n::{Arg, Locale};
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::button::{Variant, link_button};
use ferrohealth_ui::view::notice::{Announce, notice};
use ferrohealth_ui::view::refusal::{Issue, Refusal, Severity, refusal};
use ferrohealth_ui::view::surface::{card, cluster, definitions, disclosure, stack};
use ferrohealth_ui::view::time::timestamp;
use http::StatusCode;

use crate::locale::messages::Msg;
use crate::locale::{tr, tr_with};
use crate::markup::{h, t};
use crate::model::{Failure, Probe, Reach};
use crate::visit::Visit;

/// A page drawn from its visit: its title, its body, the docked decision
/// bar it may have, and the status it is answered with.
#[derive(Debug)]
pub struct Drawn {
    /// The page's words in the document title, which the console's name
    /// completes; empty for the console's name alone.
    pub title: String,
    /// The page, drawn in the shell's main area.
    pub body: Html,
    /// The docked decision bar at the foot of the main area (R4), if any.
    pub dock: Option<Html>,
    /// The status the page is answered with.
    pub status: StatusCode,
}

impl Drawn {
    /// The page titled `title` drawn as `body`, answered `200`.
    #[must_use]
    pub fn new(title: impl Into<String>, body: Html) -> Self {
        Self {
            title: title.into(),
            body,
            dock: None,
            status: StatusCode::OK,
        }
    }
}

/// The words of `failure`, as the technical detail names it.
fn failure_words(locale: &Locale, reach: &Reach) -> String {
    match reach {
        Reach::Up => String::new(),
        Reach::Refused(status) => tr_with(
            locale,
            Msg::FailureStatus,
            &[("status", Arg::from(status.as_u16().to_string()))],
        ),
        Reach::Unreachable(Failure::Connect) => tr(locale, Msg::FailureConnect),
        Reach::Unreachable(Failure::Timeout) => tr(locale, Msg::FailureTimeout),
        Reach::Unreachable(Failure::Other) => tr(locale, Msg::FailureOther),
    }
}

/// The refusal of the call `probe`, which `what` cannot be shown without.
fn probe_refusal(locale: &Locale, probe: &Probe, what: &str) -> Refusal {
    let what = Arg::from(what.to_owned());
    let (status, summary) = match &probe.reach {
        Reach::Refused(status) => (
            Some(status.as_u16()),
            tr_with(locale, Msg::RefusedCall, &[("what", what)]),
        ),
        Reach::Unreachable(Failure::Connect) => (
            None,
            tr_with(locale, Msg::UnreachableConnect, &[("what", what)]),
        ),
        Reach::Unreachable(Failure::Timeout) => (
            None,
            tr_with(locale, Msg::UnreachableTimeout, &[("what", what)]),
        ),
        Reach::Unreachable(Failure::Other) | Reach::Up => (
            None,
            tr_with(locale, Msg::UnreachableOther, &[("what", what)]),
        ),
    };
    let problem = probe.problem.iter().map(|problem| {
        let named = problem
            .title
            .clone()
            .or_else(|| problem.slug().map(str::to_owned))
            .unwrap_or_else(|| problem.kind.clone());
        let shown = Issue::new(Severity::Error, named);
        match &problem.detail {
            Some(text) => shown.text(text.clone()),
            None => shown,
        }
    });
    let issues = problem
        .chain(probe.issues.iter().map(|issue| {
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
        }))
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
fn try_again(visit: &Visit, probe: &Probe) -> Html {
    let locale = &visit.locale;
    cluster(
        link_button(
            visit.path().to_owned(),
            Variant::Secondary,
            Some(Glyph::REFRESH_CW),
            visit.tr(Msg::TryAgain),
        )
        .then(h(format!(
            "<span class=\"fui-small fui-muted\">{} {}</span>",
            t(visit.tr(Msg::LastTried)),
            timestamp(locale, &probe.at)
        ))),
    )
}

/// The technical detail of the call `probe`, folded away.
fn technical_detail(locale: &Locale, probe: &Probe) -> Html {
    disclosure(
        tr(locale, Msg::TechnicalDetail),
        definitions(vec![
            (
                tr(locale, Msg::DetailFailure),
                t(failure_words(locale, &probe.reach)),
            ),
            (
                tr(locale, Msg::DetailRoute),
                h(format!("<code>{}</code>", t(&probe.route))),
            ),
            (
                tr(locale, Msg::DetailRequestId),
                h(format!("<code>{}</code>", t(&probe.request_id))),
            ),
            (tr(locale, Msg::DetailTime), timestamp(locale, &probe.at)),
        ]),
    )
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

impl Needs {
    /// What a page needs to show `what`, saying nothing of what the
    /// console keeps.
    #[must_use]
    pub const fn of(what: Msg) -> Self {
        Self { what, away: false }
    }
}

/// The state of a page that reads the operator API `route`.
///
/// It is drawn from the call `probe` to the server's liveness route: the
/// refusal with Try again and the technical detail when the server did not
/// answer or refused, and the notice naming the route when it answers.
#[must_use]
pub fn server_state(visit: &Visit, probe: Option<&Probe>, needs: Needs, route: &str) -> Html {
    let locale = &visit.locale;
    let what = tr(locale, needs.what);
    let Some(probe) = probe else {
        return Html::empty();
    };
    if probe.is_up() {
        return notice(
            Tone::Info,
            Announce::Status,
            None,
            h(format!(
                "<p>{}</p><p class=\"fui-small fui-muted\">{}</p>",
                t(tr_with(
                    locale,
                    Msg::NotServed,
                    &[("what", Arg::from(what))]
                )),
                t(tr_with(
                    locale,
                    Msg::NotServedRoute,
                    &[("route", Arg::from(route.to_owned()))]
                ))
            )),
        );
    }
    let away = if needs.away {
        card(
            tr(locale, Msg::AwayTitle),
            h(format!("<p>{}</p>", t(tr(locale, Msg::AwayBody)))),
        )
    } else {
        Html::empty()
    };
    stack(Html::join([
        refusal(locale, &probe_refusal(locale, probe, &what)),
        try_again(visit, probe),
        away,
        technical_detail(locale, probe),
    ]))
}

/// The words of a steward action's or a source message's kind as the server
/// names it, or the name itself for a kind the console does not know.
#[must_use]
pub fn action_words(locale: &Locale, kind: &str) -> String {
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
    tr(locale, words)
}

/// The words of an administrative gender code (the FHIR R4 `ValueSet`
/// `administrative-gender`), or the code itself for one outside it.
#[must_use]
pub fn gender_words(locale: &Locale, code: &str) -> String {
    let words = match code {
        "female" => Msg::GenderFemale,
        "male" => Msg::GenderMale,
        "other" => Msg::GenderOther,
        "unknown" => Msg::GenderUnknown,
        other => return other.to_owned(),
    };
    tr(locale, words)
}

/// `value` as text, or FerroUI's muted dash for none.
#[must_use]
pub fn or_dash(locale: &Locale, value: Option<String>) -> Html {
    value.map_or_else(
        || ferrohealth_ui::view::surface::or_none(locale, String::new()),
        t,
    )
}
