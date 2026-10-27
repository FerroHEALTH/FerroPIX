// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! A merge walked from its preview and its revert from the patient's page:
//! nothing is posted before the danger dialog confirms, Escape gives the
//! focus back to the button that opened it, the merge leads to the
//! survivor whose history shows it, and Unmerge… reverts it. No
//! specification governs the console: our own design.

use thirtyfour::prelude::*;

use crate::journeys::browser::{Journey, with_journey};
use crate::journeys::journeys_enabled;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{FILLED_PERSON, Server, Stack};

/// The other patient of the merge.
const OTHER: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5e";

/// Whether the dialog `arguments[0]` is open.
const OPEN: &str = "return String(document.getElementById(arguments[0]).open);";

/// The `commandfor` of the active element, or an empty text.
const ACTIVE_FOR: &str =
    "const el = document.activeElement; return el ? (el.getAttribute('commandfor') || '') : '';";

/// Whether the page's main content draws `words`, as `yes` or `no`.
const DRAWS: &str =
    "return document.querySelector('main').innerText.includes(arguments[0]) ? 'yes' : 'no';";

/// Waits until the address's path is exactly `wanted`.
async fn at(journey: &Journey, wanted: &str) {
    for _ in 0..100 {
        let address = journey
            .driver()
            .current_url()
            .await
            .map(|url| url.path().to_owned())
            .unwrap_or_default();
        if address == wanted {
            journey.hydrated().await;
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("the address never became {wanted}");
}

/// Whether the dialog `id` is open, as `true` or `false`.
async fn open(journey: &Journey, id: &str) -> String {
    journey
        .evaluate_with(OPEN, String::from(id))
        .await
        .expect("the dialog's state")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_merge_from_its_preview_and_its_revert_from_the_patient() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        let patient = format!("/patients/{FILLED_PERSON}");
        journey.open(&stack.url(&patient)).await;
        journey
            .press(By::Css(format!("a[href='{patient}/merge']")), "Merge into…")
            .await;
        at(journey, &format!("{patient}/merge")).await;
        let other = journey
            .element(By::Id("merge-other"), "the other patient")
            .await;
        other.send_keys(OTHER).await.expect("the id");
        journey.keys(&other, Key::Enter, "the preview").await;
        let ask = journey
            .element(By::Css("button[commandfor='merge-confirm']"), "Merge…")
            .await;
        ask.focus().await.expect("the button takes the focus");
        ask.send_keys(Key::Enter)
            .await
            .expect("Enter opens the dialog");
        assert_eq!(
            "true",
            open(journey, "merge-confirm").await,
            "Merge… asks first"
        );
        journey
            .active()
            .await
            .send_keys(Key::Escape)
            .await
            .expect("Escape");
        assert_eq!(
            "false",
            open(journey, "merge-confirm").await,
            "Escape closes it"
        );
        assert_eq!(
            "merge-confirm",
            journey.evaluate(ACTIVE_FOR).await.expect("the focus"),
            "the focus is back on Merge…"
        );
        assert!(
            journey
                .driver()
                .current_url()
                .await
                .is_ok_and(|url| url.path().ends_with("/merge")),
            "nothing was posted"
        );
        ask.send_keys(Key::Enter)
            .await
            .expect("Enter opens the dialog");
        let reason = journey.element(By::Id("merge-reason"), "the reason").await;
        reason
            .send_keys("Synthetic ward call")
            .await
            .expect("the reason");
        journey.keys(&reason, Key::Enter, "the merge posted").await;
        at(journey, &patient).await;
        journey
            .open(&stack.url(&format!("{patient}?tab=history")))
            .await;
        assert_eq!(
            "yes",
            journey
                .evaluate_with(DRAWS, String::from("Merged"))
                .await
                .expect("the history"),
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
        journey.keys(&reason, Key::Enter, "the revert posted").await;
        at(journey, &patient).await;
    })
    .await;
}
