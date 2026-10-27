// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A pair compared and decided from the keyboard: `l` asks in the danger
//! dialog a link that merges needs, and Escape gives the focus back to its
//! button; `n` with no reason puts the focus in the reason, which `r` also
//! reaches; `n` with a reason posts the decision and the next pair opens;
//! `[` goes back. No specification governs the console: our own design.

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

/// Takes the focus out of any field, onto the page.
const BLUR: &str = "if (document.activeElement) { document.activeElement.blur(); } return 'done';";

/// Presses `key` on the page's body, outside every field.
async fn press_key(journey: &crate::journeys::browser::Journey, key: &str) {
    journey
        .evaluate(BLUR)
        .await
        .expect("the focus leaves the field");
    journey
        .element(By::Css("body"), "the page")
        .await
        .send_keys(key)
        .await
        .expect("the page takes the key");
}

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
async fn the_decision_keys_ask_hold_post_and_walk_the_pairs() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        journey
            .open(&stack.url(&format!("/queue/{FILLED_CANDIDATE}")))
            .await;
        press_key(journey, "l").await;
        assert_eq!(
            "true",
            journey.evaluate(MERGE_OPEN).await.expect("the dialog"),
            "l asks in the merge dialog"
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
        press_key(journey, "r").await;
        assert_eq!(
            "decide-reason",
            journey.evaluate(ACTIVE_FOR).await.expect("the focus"),
            "r puts the focus in the reason"
        );
        press_key(journey, "n").await;
        journey
            .count_becoming(
                By::Css("#decide-reason[aria-invalid='true']"),
                1,
                "the reason's error",
            )
            .await;
        let reason = journey.element(By::Id("decide-reason"), "the reason").await;
        reason
            .send_keys("Synthetic ward call")
            .await
            .expect("the reason");
        press_key(journey, "n").await;
        journey
            .address_carrying(&format!("/queue/{NEXT}"), "the next pair")
            .await;
        journey
            .open(&stack.url(&format!("/queue/{FILLED_CANDIDATE}")))
            .await;
        press_key(journey, "]").await;
        journey
            .address_carrying(&format!("/queue/{NEXT}"), "] leads to the next pair")
            .await;
    })
    .await;
}
