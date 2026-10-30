// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The parts of the shell every page shares.
//!
//! They are the footer, the lockup, the navigation of
//! `docs/architecture.md` §12.2 as the design groups it, and the top bar's
//! status pill. No specification governs the shell: our own design, on
//! FerroUI's.

use ferrohealth_ui::html::Html;
use ferrohealth_ui::i18n::{Arg, Locale};
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::view::nav::{Count, Entry, Group};
use ferrohealth_ui::view::shell::{Footer, brand_mark, lockup};
use ferrohealth_ui::view::status::{Reach as PillReach, StatusPill, status_pill};

use super::{
    ABOUT, AUDIT, CONFIGURATION, DASHBOARD, DOMAINS, PSEUDONYM_DOMAINS, QUEUE, SEARCH, SOURCES,
    VERSION,
};
use crate::locale::messages::Msg;
use crate::locale::{tr, tr_with};
use crate::model::{Probe, Reach};
use crate::paths::{MARK_DARK, MARK_LIGHT};

/// The footer every page carries.
#[must_use]
pub fn footer(locale: &Locale) -> Footer {
    Footer {
        product: tr(locale, Msg::ConsoleName).into(),
        version: VERSION.into(),
        about: ABOUT.into(),
    }
}

/// The FerroPIX lockup, its marks in both themes, leading to the
/// dashboard.
#[must_use]
pub fn brand_lockup(locale: &Locale) -> Html {
    lockup(
        DASHBOARD,
        brand_mark(MARK_LIGHT, MARK_DARK),
        tr(locale, Msg::Product),
        tr(locale, Msg::ConsoleWord),
    )
}

/// One entry of the navigation.
fn entry(locale: &Locale, href: &'static str, label: Msg, glyph: Glyph, exact: bool) -> Entry {
    Entry {
        href: href.into(),
        label: tr(locale, label).into(),
        glyph,
        exact,
        count: None,
    }
}

/// The navigation's groups, as the design draws them.
pub(super) fn navigation(locale: &Locale) -> Vec<Group> {
    // NOTE: FerroUI COMPONENTS.md R12: a count the console does not know is a muted
    // dash with its reason, never a zero; the queue's depth is the operator API's.
    let queue = Entry {
        count: Some(Count::Unknown {
            why: tr(locale, Msg::QueueDepthUnknown),
        }),
        ..entry(locale, QUEUE, Msg::Queue, Glyph::LIST_CHECKS, false)
    };
    vec![
        Group {
            id: "nav-work",
            title: tr(locale, Msg::NavWork).into(),
            entries: vec![
                entry(
                    locale,
                    DASHBOARD,
                    Msg::Dashboard,
                    Glyph::LAYOUT_DASHBOARD,
                    true,
                ),
                queue,
            ],
        },
        Group {
            id: "nav-patients",
            title: tr(locale, Msg::NavPatients).into(),
            entries: vec![entry(
                locale,
                SEARCH,
                Msg::Search,
                Glyph::USER_ROUND_SEARCH,
                false,
            )],
        },
        Group {
            id: "nav-registry",
            title: tr(locale, Msg::NavRegistry).into(),
            entries: vec![
                entry(locale, SOURCES, Msg::Sources, Glyph::RADIO_TOWER, false),
                entry(locale, DOMAINS, Msg::Domains, Glyph::TAGS, false),
                entry(
                    locale,
                    PSEUDONYM_DOMAINS,
                    Msg::PseudonymDomains,
                    Glyph::SHIELD_CHECK,
                    false,
                ),
            ],
        },
        Group {
            id: "nav-trail",
            title: tr(locale, Msg::NavTrail).into(),
            entries: vec![entry(locale, AUDIT, Msg::Audit, Glyph::FILE_CLOCK, false)],
        },
        Group {
            id: "nav-console",
            title: tr(locale, Msg::NavConsole).into(),
            entries: vec![
                entry(
                    locale,
                    CONFIGURATION,
                    Msg::Configuration,
                    Glyph::FILE_TEXT,
                    false,
                ),
                entry(locale, ABOUT, Msg::About, Glyph::INFO, true),
            ],
        },
    ]
}

/// The top bar's status pill for the call `probe` to the server's liveness
/// route.
pub(super) fn pill(locale: &Locale, probe: Option<&Probe>, base: &str) -> Html {
    let (reach, words) = match probe.map(|probe| &probe.reach) {
        Some(Reach::Up) => (PillReach::Up, tr(locale, Msg::PillUp)),
        Some(Reach::Refused(status))
            if *status == http::StatusCode::UNAUTHORIZED
                || *status == http::StatusCode::FORBIDDEN =>
        {
            (
                PillReach::Refused,
                tr_with(
                    locale,
                    Msg::PillRefused,
                    &[("status", Arg::from(status.as_u16().to_string()))],
                ),
            )
        }
        Some(Reach::Refused(status)) => (
            PillReach::Degraded,
            tr_with(
                locale,
                Msg::PillDegraded,
                &[("status", Arg::from(status.as_u16().to_string()))],
            ),
        ),
        Some(Reach::Unreachable(_)) | None => {
            (PillReach::Unreachable, tr(locale, Msg::PillUnreachable))
        }
    };
    status_pill(StatusPill {
        reach,
        words,
        detail: Some(tr_with(
            locale,
            Msg::PillDetail,
            &[("base", Arg::from(base.to_owned()))],
        )),
    })
}
