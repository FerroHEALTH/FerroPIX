// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The review queue walked from the keyboard: `j` and `k` move the focused
//! row, `o` opens its side panel and Escape closes it, `d` opens it to
//! defer, the deferral is posted in place, and Enter opens the pair. No
//! specification governs the console: our own design.

use thirtyfour::prelude::*;

use crate::journeys::browser::with_journey;
use crate::journeys::journeys_enabled;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{FILLED_CANDIDATE, Server, Stack};

/// The key of the row that holds the focus, or an empty text.
const FOCUSED_ROW: &str = "const row = document.activeElement; return row && row.matches('tr[data-key]') ? row.dataset.key : '';";

/// Puts the focus on the first row and returns its key.
const FIRST_ROW: &str = "const row = document.querySelector('.fui-worklist-grid tr[data-key]'); row.focus(); return row.dataset.key;";

/// Whether the first row's side panel is open.
const PANEL_OPEN: &str = "const panel = document.getElementById('glance-' + arguments[0]); return String(Boolean(panel && panel.open));";

/// Whether the page's main content draws `words`, as `yes` or `no`.
const DRAWS: &str =
    "return document.querySelector('main').innerText.includes(arguments[0]) ? 'yes' : 'no';";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn j_k_o_d_and_enter_walk_the_queue_defer_a_pair_and_open_one() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        journey.open(&stack.url("/queue")).await;
        journey
            .count_becoming(
                By::Css(".fui-worklist-grid tr[data-key]"),
                3,
                "three candidates",
            )
            .await;
        let first = journey.evaluate(FIRST_ROW).await.expect("the first row");
        assert_eq!(FILLED_CANDIDATE, first);
        journey.active().await.send_keys("j").await.expect("j");
        let second = journey.evaluate(FOCUSED_ROW).await.expect("the row");
        assert_ne!(first, second, "j moves to the next row");
        journey.active().await.send_keys("k").await.expect("k");
        assert_eq!(
            first,
            journey.evaluate(FOCUSED_ROW).await.expect("the row"),
            "k moves back"
        );
        journey.active().await.send_keys("o").await.expect("o");
        assert_eq!(
            "true",
            journey
                .evaluate_with(PANEL_OPEN, first.clone())
                .await
                .expect("the panel's state"),
            "o opens the focused row's side panel"
        );
        journey
            .active()
            .await
            .send_keys(Key::Escape)
            .await
            .expect("Escape");
        assert_eq!(
            "false",
            journey
                .evaluate_with(PANEL_OPEN, first.clone())
                .await
                .expect("the panel's state"),
            "Escape closes it"
        );
        journey.evaluate(FIRST_ROW).await.expect("the first row");
        journey.active().await.send_keys("d").await.expect("d");
        let until = journey
            .element(By::Id(format!("until-{first}")), "the deferral's date")
            .await;
        until.send_keys("01-12-2099").await.expect("the date");
        let reason = journey
            .element(By::Id(format!("reason-{first}")), "the deferral's reason")
            .await;
        reason
            .send_keys("Synthetic ward call")
            .await
            .expect("the reason");
        journey
            .keys(&reason, Key::Enter, "the deferral posted")
            .await;
        journey
            .address_carrying("/queue", "the queue's own address")
            .await;
        let drawn = journey
            .evaluate_with(DRAWS, String::from("The pair is deferred"))
            .await
            .expect("the page's words");
        assert_eq!("yes", drawn, "the deferral is confirmed in place");
        let first = journey.evaluate(FIRST_ROW).await.expect("the first row");
        journey
            .keys(&journey.active().await, Key::Enter, "the pair's page")
            .await;
        journey
            .address_carrying(&format!("/queue/{first}"), "Enter opens the pair")
            .await;
    })
    .await;
}
