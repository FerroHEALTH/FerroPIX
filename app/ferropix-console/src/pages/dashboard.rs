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

use ferrohealth_health::document::State;
use ferrohealth_ui::html::Html;
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::tone::Tone;
use ferrohealth_ui::view::chart::{BAD, ChartWords, INFO, LineChart, OK, Series, WARN, line_chart};
use ferrohealth_ui::view::state::{Mark, state};
use ferrohealth_ui::view::status::badge;
use ferrohealth_ui::view::surface::{
    Stat, card, card_action, grid, stack, stat_card_marked, stat_grid,
};
use ferrohealth_ui::view::template::list::list_empty;
use ferrohealth_ui::view::time::timestamp;
use ferropix_operator_api::health::Dependencies;
use ferropix_operator_api::overview::Overview;

use crate::app::QUEUE;
use crate::locale::messages::Msg;
use crate::markup::{h, t};
use crate::model::Read;
use crate::pages::queue::grade_words;
use crate::pages::{Needs, server_state};
use crate::visit::Visit;

/// The readiness indicator of the MLLP listener.
pub const MLLP_INDICATOR: &str = "hl7v2_listener";

/// The readiness indicator of the outbound feed sender.
pub const FEED_INDICATOR: &str = "feed_sender";

/// The dashboard over what the server answered.
#[must_use]
pub fn dashboard(
    visit: &Visit,
    overview: &Read<Overview>,
    dependencies: &Read<Dependencies>,
) -> Html {
    let needs = Needs {
        what: Msg::WhatDashboard,
        away: true,
    };
    let body = match overview {
        Ok(overview) => Html::join([
            stats(visit, overview),
            grid(
                card(visit.tr(Msg::QueueDepth), queue_chart(visit, overview)).then(card(
                    visit.tr(Msg::FeedsPerHour),
                    feeds_chart(visit, overview),
                )),
            ),
            grid(listeners(visit, dependencies).then(oldest(visit, overview))),
        ]),
        Err(probe) => server_state(visit, Some(probe), needs, "GET /operator/overview")
            .then(listeners(visit, dependencies)),
    };
    stack(body)
}

/// The four stat cards.
fn stats(visit: &Visit, overview: &Overview) -> Html {
    let waiting = overview.queue.waiting_over_week;
    let queue = stat_card_marked(
        Stat {
            glyph: Glyph::INBOX,
            value: visit.tr_with(Msg::Count, &[("count", overview.queue.open.into())]),
            label: Cow::Owned(visit.tr(Msg::StatOpenQueue)),
            href: Some(String::from(QUEUE)),
        },
        if waiting > 0 {
            badge(
                Tone::Warn,
                visit.tr_with(Msg::StatWaitingOverWeek, &[("count", waiting.into())]),
            )
        } else {
            Html::empty()
        },
    );
    let rate = if overview.records_created_24h == 0 {
        String::from("–")
    } else {
        let percent = overview
            .records_linked_24h
            .saturating_mul(100)
            .checked_div(overview.records_created_24h)
            .unwrap_or(0);
        visit.tr_with(Msg::Percent, &[("percent", percent.into())])
    };
    let linked = stat_card_marked(
        Stat {
            glyph: Glyph::LINK,
            value: rate,
            label: Cow::Owned(visit.tr(Msg::StatAutoLink)),
            href: None,
        },
        badge(
            Tone::Neutral,
            visit.tr_with(
                Msg::StatCreated,
                &[("count", overview.records_created_24h.into())],
            ),
        ),
    );
    let fed = stat_card_marked(
        Stat {
            glyph: Glyph::ARROW_DOWN,
            value: visit.tr_with(Msg::Count, &[("count", overview.versions_fed_today.into())]),
            label: Cow::Owned(visit.tr(Msg::StatFedToday)),
            href: None,
        },
        Html::empty(),
    );
    let errors = overview.subscriptions_in_error;
    let subscriptions = stat_card_marked(
        Stat {
            glyph: if errors > 0 {
                Glyph::CIRCLE_X
            } else {
                Glyph::CIRCLE_CHECK
            },
            value: visit.tr_with(Msg::Count, &[("count", errors.into())]),
            label: Cow::Owned(visit.tr(Msg::StatSubscriptions)),
            href: None,
        },
        if errors > 0 {
            badge(Tone::Bad, visit.tr(Msg::StatInError))
        } else {
            Html::empty()
        },
    );
    stat_grid(Html::join([queue, linked, fed, subscriptions]))
}

/// The queue's depth over the last days as a line chart.
fn queue_chart(visit: &Visit, overview: &Overview) -> Html {
    if overview.queue_depth.is_empty() {
        return list_empty(visit.tr(Msg::QueueDepthEmpty), None);
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
        label: visit.tr(Msg::QueueDepth),
        summary: visit.tr_with(
            Msg::QueueDepthSummary,
            &[("days", overview.queue_depth.len().into())],
        ),
        series: vec![Series {
            name: visit.tr(Msg::QueueOpen),
            points,
            palette: INFO,
        }],
        x_ticks: ticks,
        x_labels: labels,
        y_range: None,
    };
    line_chart(&chart, ChartWords::of(&visit.locale))
}

/// The feeds per hour by channel as a line chart, a line per channel in its
/// tone.
fn feeds_chart(visit: &Visit, overview: &Overview) -> Html {
    if overview.feeds_per_hour.is_empty() {
        return list_empty(visit.tr(Msg::FeedsEmpty), None);
    }
    let line =
        |name: Msg, palette: u8, count: fn(&ferropix_operator_api::overview::HourFeeds) -> u64| {
            Series {
                name: visit.tr(name),
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
        label: visit.tr(Msg::FeedsPerHour),
        summary: visit.tr_with(
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
    line_chart(&chart, ChartWords::of(&visit.locale))
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
fn listeners(visit: &Visit, dependencies: &Read<Dependencies>) -> Html {
    let body = match dependencies {
        Err(probe) => server_state(
            visit,
            Some(probe),
            Needs::of(Msg::WhatListeners),
            "GET /operator/dependencies",
        ),
        Ok(dependencies) => {
            let indicator = |name: &str| {
                dependencies
                    .readiness
                    .indicators
                    .iter()
                    .find(|indicator| indicator.name == name)
            };
            let row = |words: Msg, mark: Html| {
                h(format!(
                    "<li class=\"fui-cluster\"><span>{}</span>{mark}</li>",
                    t(visit.tr(words))
                ))
            };
            let of = |name: &str| match indicator(name) {
                Some(found) if found.state == State::Up => {
                    state(Mark::Ok, visit.tr(Msg::ListenerUp), None)
                }
                Some(found) => state(Mark::Bad, visit.tr(Msg::ListenerDown), found.detail.clone()),
                None => state(Mark::Neutral, visit.tr(Msg::ListenerOff), None),
            };
            h(format!(
                "<ul class=\"fui-plain-list\">{}{}{}</ul>",
                row(
                    Msg::ListenerHttp,
                    state(Mark::Ok, visit.tr(Msg::ListenerUp), None)
                ),
                row(Msg::ListenerMllp, of(MLLP_INDICATOR)),
                row(Msg::ListenerFeed, of(FEED_INDICATOR)),
            ))
        }
    };
    card(visit.tr(Msg::Listeners), body)
}

/// The oldest pairs in the queue, with the way to the queue.
fn oldest(visit: &Visit, overview: &Overview) -> Html {
    let body = if overview.oldest.is_empty() {
        list_empty(visit.tr(Msg::QueueEmpty), None)
    } else {
        let rows: Html = overview
            .oldest
            .iter()
            .map(|entry| {
                h(format!(
                    "<li class=\"fui-cluster\"><a href=\"{}\">{}</a>{}<span class=\"fui-muted fui-small\">{}</span></li>",
                    t(format!("{QUEUE}/{}", entry.candidate)),
                    timestamp(&visit.locale, &entry.created_at),
                    badge(Tone::Info, grade_words(visit, entry.grade)),
                    t(visit.tr_with(
                        Msg::Score,
                        &[("score", format!("{:.2}", entry.score).into())]
                    )),
                ))
            })
            .collect();
        h(format!("<ul class=\"fui-plain-list\">{rows}</ul>"))
    };
    card(
        visit.tr(Msg::OldestInQueue),
        body.then(card_action(QUEUE, visit.tr(Msg::OpenQueue))),
    )
}
