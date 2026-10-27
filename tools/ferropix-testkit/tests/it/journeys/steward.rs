// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The steward's work end to end: the queue's oldest pair opened, its
//! compare and breakdown read, a decision made with a reason and the next
//! pair opened, then two patients merged from the preview and the merge
//! reverted from the patient's page. No specification governs the
//! console: our own design.

use thirtyfour::prelude::*;

use crate::journeys::browser::{Journey, with_journey};
use crate::journeys::journeys_enabled;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{FILLED_CANDIDATE, FILLED_PERSON, Server, Stack};

/// The pair after the oldest in the filled queue.
const NEXT: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4b02";

/// The other patient of the merge.
const OTHER: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5e";

/// Whether the page's main content draws `words`, as `yes` or `no`.
const DRAWS: &str =
    "return document.querySelector('main').innerText.includes(arguments[0]) ? 'yes' : 'no';";

/// Waits until the address's path is exactly `wanted`.
async fn at(journey: &Journey, wanted: &str) {
    for _ in 0..100 {
        let path = journey
            .driver()
            .current_url()
            .await
            .map(|url| url.path().to_owned())
            .unwrap_or_default();
        if path == wanted {
            journey.hydrated().await;
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("the address never became {wanted}");
}

/// Whether the page draws `words`.
async fn draws(journey: &Journey, words: &str) -> bool {
    journey
        .evaluate_with(DRAWS, String::from(words))
        .await
        .is_ok_and(|answer| answer == "yes")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_steward_works_the_queue_merges_and_reverts() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        journey.open(&stack.url("/queue")).await;
        let first = journey
            .element(
                By::Css(".fui-worklist-grid tr[data-key]"),
                "the oldest pair",
            )
            .await;
        first.focus().await.expect("the row takes the focus");
        journey.keys(&first, Key::Enter, "the pair").await;
        at(journey, &format!("/queue/{FILLED_CANDIDATE}")).await;
        assert!(draws(journey, "The two records").await, "the compare");
        assert!(
            draws(journey, "Why the pair scored this").await,
            "the breakdown"
        );
        let reason = journey.element(By::Id("decide-reason"), "the reason").await;
        reason
            .send_keys("Synthetic ward call")
            .await
            .expect("the reason");
        journey
            .press(By::Css("button[value='not-a-match']"), "Not a match")
            .await;
        at(journey, &format!("/queue/{NEXT}")).await;
        let patient = format!("/patients/{FILLED_PERSON}");
        journey.open(&stack.url(&format!("{patient}/merge"))).await;
        let other = journey
            .element(By::Id("merge-other"), "the other patient")
            .await;
        other.send_keys(OTHER).await.expect("the id");
        journey.keys(&other, Key::Enter, "the preview").await;
        assert!(draws(journey, "What the merge does").await, "the preview");
        journey
            .press(By::Css("button[commandfor='merge-confirm']"), "Merge…")
            .await;
        let reason = journey.element(By::Id("merge-reason"), "the reason").await;
        reason
            .send_keys("Synthetic ward call")
            .await
            .expect("the reason");
        journey.keys(&reason, Key::Enter, "the merge").await;
        at(journey, &patient).await;
        journey
            .open(&stack.url(&format!("{patient}?tab=history")))
            .await;
        assert!(
            draws(journey, "Merged").await,
            "the history shows the merge"
        );
        journey
            .press(By::Css(format!("a[href='{patient}/unmerge']")), "Unmerge…")
            .await;
        at(journey, &format!("{patient}/unmerge")).await;
        journey
            .press(By::Css("button[commandfor='revert-confirm']"), "Unmerge…")
            .await;
        let reason = journey.element(By::Id("revert-reason"), "the reason").await;
        reason
            .send_keys("Synthetic mistake")
            .await
            .expect("the reason");
        journey.keys(&reason, Key::Enter, "the revert").await;
        at(journey, &patient).await;
    })
    .await;
}
