// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A pair compared and decided: Link… asks in the danger dialog a link
//! that merges needs, and Escape gives the focus back to its button; Not a
//! match with no reason comes back with the reason marked and focused; Not
//! a match with a reason posts the decision and the next pair opens; Next
//! leads to it too. The keys of 0.5 (`l`, `n`, `r`, `[` and `]`) come back
//! with #390. No specification governs the console: our own design.

use thirtyfour::prelude::*;

use crate::journeys::browser::with_journey;
use crate::journeys::journeys_enabled;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{FILLED_CANDIDATE, Server, Stack};

/// The pair after the first in the filled queue.
const NEXT: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4b02";

/// Whether the merge dialog is open.
const MERGE_OPEN: &str = "return String(document.getElementById('link-merges').open);";

/// The `commandfor` of the active element, or an empty text.
const ACTIVE_FOR: &str = "const el = document.activeElement; return el ? (el.getAttribute('commandfor') || el.id || '') : '';";

/// The focused element's `commandfor` or id, once it is `wanted` or after
/// five seconds.
async fn focus_becoming(journey: &crate::journeys::browser::Journey, wanted: &str) -> String {
    let mut seen = String::new();
    for _ in 0..50 {
        seen = journey.evaluate(ACTIVE_FOR).await.expect("the focus");
        if seen == wanted {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    seen
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pair_asks_holds_posts_and_walks_to_the_next() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        journey
            .open(&stack.url(&format!("/queue/{FILLED_CANDIDATE}")))
            .await;
        journey
            .press(By::Css("button[commandfor='link-merges']"), "Link…")
            .await;
        assert_eq!(
            "true",
            journey.evaluate(MERGE_OPEN).await.expect("the dialog"),
            "Link… asks in the merge dialog"
        );
        journey
            .active()
            .await
            .send_keys(Key::Escape)
            .await
            .expect("Escape");
        assert_eq!(
            "false",
            journey.evaluate(MERGE_OPEN).await.expect("the dialog"),
            "Escape closes it"
        );
        assert_eq!(
            "link-merges",
            focus_becoming(journey, "link-merges").await,
            "the focus is back on Link…"
        );
        journey
            .press(
                By::Css("button[value='not-a-match']"),
                "Not a match with no reason",
            )
            .await;
        journey
            .count_becoming(
                By::Css("#decide-reason[aria-invalid='true']"),
                1,
                "the reason's error",
            )
            .await;
        assert_eq!(
            "decide-reason",
            focus_becoming(journey, "decide-reason").await,
            "the focus is in the reason"
        );
        let reason = journey.element(By::Id("decide-reason"), "the reason").await;
        reason
            .send_keys("Synthetic ward call")
            .await
            .expect("the reason");
        journey
            .press(By::Css("button[value='not-a-match']"), "Not a match")
            .await;
        journey
            .address_carrying(&format!("/queue/{NEXT}"), "the next pair")
            .await;
        journey
            .open(&stack.url(&format!("/queue/{FILLED_CANDIDATE}")))
            .await;
        journey
            .press(By::Css(format!("a[href='/queue/{NEXT}']")), "Next")
            .await;
        journey
            .address_carrying(&format!("/queue/{NEXT}"), "Next leads to the next pair")
            .await;
    })
    .await;
}
