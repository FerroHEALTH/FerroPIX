// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The dashboard at `/` (design 01, 16): the four stat cards, the queue's
//! depth and the feeds per hour as charts, the listeners and the oldest
//! pairs in the queue.
//!
//! The cards read `GET /operator/overview` and the listeners card
//! `GET /operator/dependencies`; each draws its own refusal in place, so
//! one failed read never blanks the page. The charts are FerroUI's SVG line
//! charts with their accessible names and their figures tables. No
//! specification governs the console: our own design.

use std::borrow::Cow;

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::chart::{BAD, INFO, LineChart, OK, Series, WARN};
use ferrohealth_ui::view::state::{Mark, state};
use ferrohealth_ui::view::status::badge;
use ferrohealth_ui::view::surface::{
    Stat, card, card_action, grid, stack, stat_card_marked, stat_grid,
};
use ferrohealth_ui::view::template::list::list_empty;
use ferrohealth_ui::view::time::timestamp;
use ferrohealth_ui::widget::chart::{ChartWords, line_chart};
use ferropix_operator_api::health::{Dependencies, State};
use ferropix_operator_api::overview::Overview;
use ferropix_operator_api::patient::Grade;
use leptos::prelude::*;

use crate::app::QUEUE;
use crate::document::Read;
use crate::locale::{Msg, tr, tr_with};
use crate::pages::{Needs, server_state};

/// The readiness indicator of the MLLP listener.
pub const MLLP_INDICATOR: &str = "hl7v2_listener";

/// The readiness indicator of the outbound feed sender.
pub const FEED_INDICATOR: &str = "feed_sender";

/// The dashboard over what the server answered.
#[must_use]
pub fn dashboard(overview: &Read<Overview>, dependencies: &Read<Dependencies>) -> AnyView {
    let needs = Needs {
        what: Msg::WhatDashboard,
        away: true,
    };
    let body = match overview {
        Ok(overview) => view! {
            {stats(overview)}
            {grid(view! {
                {card(tr(Msg::QueueDepth), queue_chart(overview))}
                {card(tr(Msg::FeedsPerHour), feeds_chart(overview))}
            })}
            {grid(view! {
                {listeners(dependencies)}
                {oldest(overview)}
            })}
        }
        .into_any(),
        Err(probe) => view! {
            {server_state(Some(probe), needs, "GET /operator/overview")}
            {listeners(dependencies)}
        }
        .into_any(),
    };
    stack(body).into_any()
}

/// The four stat cards.
fn stats(overview: &Overview) -> AnyView {
    let waiting = overview.queue.waiting_over_week;
    let queue = stat_card_marked(
        Stat {
            glyph: Glyph::INBOX,
            value: tr_with(Msg::Count, &[("count", overview.queue.open.into())]),
            label: Cow::Owned(tr(Msg::StatOpenQueue)),
            href: Some(String::from(QUEUE)),
        },
        (waiting > 0).then(|| {
            badge(
                Tone::Warn,
                tr_with(Msg::StatWaitingOverWeek, &[("count", waiting.into())]),
            )
        }),
    )
    .into_any();
    let rate = if overview.records_created_24h == 0 {
        String::from("–")
    } else {
        let percent = overview
            .records_linked_24h
            .saturating_mul(100)
            .checked_div(overview.records_created_24h)
            .unwrap_or(0);
        tr_with(Msg::Percent, &[("percent", percent.into())])
    };
    let linked = stat_card_marked(
        Stat {
            glyph: Glyph::LINK,
            value: rate,
            label: Cow::Owned(tr(Msg::StatAutoLink)),
            href: None,
        },
        badge(
            Tone::Neutral,
            tr_with(
                Msg::StatCreated,
                &[("count", overview.records_created_24h.into())],
            ),
        ),
    )
    .into_any();
    let fed = stat_card_marked(
        Stat {
            glyph: Glyph::ARROW_DOWN,
            value: tr_with(Msg::Count, &[("count", overview.versions_fed_today.into())]),
            label: Cow::Owned(tr(Msg::StatFedToday)),
            href: None,
        },
        (),
    )
    .into_any();
    let errors = overview.subscriptions_in_error;
    let subscriptions = stat_card_marked(
        Stat {
            glyph: if errors > 0 {
                Glyph::CIRCLE_X
            } else {
                Glyph::CIRCLE_CHECK
            },
            value: tr_with(Msg::Count, &[("count", errors.into())]),
            label: Cow::Owned(tr(Msg::StatSubscriptions)),
            href: None,
        },
        (errors > 0).then(|| badge(Tone::Bad, tr(Msg::StatInError))),
    )
    .into_any();
    stat_grid(view! { {queue} {linked} {fed} {subscriptions} }).into_any()
}

/// The queue's depth over the last days as a line chart.
fn queue_chart(overview: &Overview) -> AnyView {
    if overview.queue_depth.is_empty() {
        return list_empty(tr(Msg::QueueDepthEmpty), None);
    }
    let points: Vec<(f64, f64)> = overview
        .queue_depth
        .iter()
        .enumerate()
        .map(|(index, day)| (as_f64(index), as_f64_count(day.open)))
        .collect();
    let labels: Vec<(f64, String)> = overview
        .queue_depth
        .iter()
        .enumerate()
        .map(|(index, day)| (as_f64(index), day.day.clone()))
        .collect();
    let ticks = ends(&labels);
    let chart = LineChart {
        id: "queue-depth",
        label: tr(Msg::QueueDepth),
        summary: tr_with(
            Msg::QueueDepthSummary,
            &[("days", overview.queue_depth.len().into())],
        ),
        series: vec![Series {
            name: tr(Msg::QueueOpen),
            points,
            palette: INFO,
        }],
        x_ticks: ticks,
        x_labels: labels,
        y_range: None,
    };
    line_chart(&chart, ChartWords::of_page())
}

/// The feeds per hour by channel as a line chart, a line per channel in its
/// tone.
fn feeds_chart(overview: &Overview) -> AnyView {
    if overview.feeds_per_hour.is_empty() {
        return list_empty(tr(Msg::FeedsEmpty), None);
    }
    let line =
        |name: Msg, palette: u8, count: fn(&ferropix_operator_api::overview::HourFeeds) -> u64| {
            Series {
                name: tr(name),
                points: overview
                    .feeds_per_hour
                    .iter()
                    .enumerate()
                    .map(|(index, hour)| (as_f64(index), as_f64_count(count(hour))))
                    .collect(),
                palette,
            }
        };
    let labels: Vec<(f64, String)> = overview
        .feeds_per_hour
        .iter()
        .enumerate()
        .map(|(index, hour)| (as_f64(index), hour.hour.clone()))
        .collect();
    let chart = LineChart {
        id: "feeds-per-hour",
        label: tr(Msg::FeedsPerHour),
        summary: tr_with(
            Msg::FeedsSummary,
            &[("hours", overview.feeds_per_hour.len().into())],
        ),
        series: vec![
            line(Msg::ChannelFhir, OK, |hour| hour.fhir),
            line(Msg::ChannelV2, WARN, |hour| hour.v2),
            line(Msg::ChannelPmir, BAD, |hour| hour.pmir),
        ],
        x_ticks: ends(&labels),
        x_labels: labels,
        y_range: None,
    };
    line_chart(&chart, ChartWords::of_page())
}

/// The first and the last of `labels`, the ticks a short axis names.
fn ends(labels: &[(f64, String)]) -> Vec<(f64, String)> {
    match (labels.first(), labels.last()) {
        (Some(first), Some(last)) if first.0 < last.0 => vec![first.clone(), last.clone()],
        (Some(first), _) => vec![first.clone()],
        _ => Vec::new(),
    }
}

/// A position as a chart coordinate.
fn as_f64(index: usize) -> f64 {
    f64::from(u32::try_from(index).unwrap_or(u32::MAX))
}

/// A count as a chart coordinate.
fn as_f64_count(count: u64) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

/// The listeners card: the HTTP listener that answered, the MLLP listener
/// and the feed sender, each from its readiness indicator.
fn listeners(dependencies: &Read<Dependencies>) -> AnyView {
    let body = match dependencies {
        Err(probe) => server_state(
            Some(probe),
            Needs {
                what: Msg::WhatListeners,
                away: false,
            },
            "GET /operator/dependencies",
        ),
        Ok(dependencies) => {
            let indicator = |name: &str| dependencies.readiness.indicators.get(name);
            let row = |words: Msg, mark: AnyView| {
                view! {
                    <li class="fui-cluster">
                        <span>{tr(words)}</span>
                        {mark}
                    </li>
                }
            };
            let of = |name: &str| match indicator(name) {
                Some(found) if found.state == State::Up => {
                    state(Mark::Ok, tr(Msg::ListenerUp), None)
                }
                Some(found) => state(Mark::Bad, tr(Msg::ListenerDown), found.detail.clone()),
                None => state(Mark::Neutral, tr(Msg::ListenerOff), None),
            };
            view! {
                <ul class="fui-plain-list">
                    {row(Msg::ListenerHttp, state(Mark::Ok, tr(Msg::ListenerUp), None))}
                    {row(Msg::ListenerMllp, of(MLLP_INDICATOR))}
                    {row(Msg::ListenerFeed, of(FEED_INDICATOR))}
                </ul>
            }
            .into_any()
        }
    };
    card(tr(Msg::Listeners), body).into_any()
}

/// The words of a grade.
fn grade(grade: Grade) -> String {
    tr(match grade {
        Grade::Certain => Msg::GradeCertain,
        Grade::Probable => Msg::GradeProbable,
        Grade::Possible => Msg::GradePossible,
        Grade::CertainlyNot => Msg::GradeCertainlyNot,
    })
}

/// The oldest pairs in the queue, with the way to the queue.
fn oldest(overview: &Overview) -> AnyView {
    let body = if overview.oldest.is_empty() {
        list_empty(tr(Msg::QueueEmpty), None)
    } else {
        let rows = overview
            .oldest
            .iter()
            .map(|entry| {
                view! {
                    <li class="fui-cluster">
                        <a href=format!("{QUEUE}/{}", entry.candidate)>{timestamp(&entry.created_at)}</a>
                        {badge(Tone::Info, grade(entry.grade))}
                        <span class="fui-muted fui-small">
                            {tr_with(Msg::Score, &[("score", format!("{:.2}", entry.score).into())])}
                        </span>
                    </li>
                }
            })
            .collect_view();
        view! { <ul class="fui-plain-list">{rows}</ul> }.into_any()
    };
    card(
        tr(Msg::OldestInQueue),
        view! {
            {body}
            {card_action(QUEUE, tr(Msg::OpenQueue))}
        },
    )
    .into_any()
}
