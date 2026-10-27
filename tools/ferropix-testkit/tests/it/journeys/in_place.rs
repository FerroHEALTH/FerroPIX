// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The hydrated console keeps its document: a link and a patient search are
//! drawn in the page the browser already holds, with no new document load.
//! A mark set on the window before the press survives it only when the
//! browser loaded no new document (the HTML Living Standard, "Navigating
//! across documents", gives each loaded document a new `Window`). No
//! specification governs the console's navigation: our own design, on
//! FerroUI's whole-page hydration.

use thirtyfour::prelude::*;

use crate::journeys::browser::with_journey;
use crate::journeys::journeys_enabled;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{Server, Stack};

/// Sets the mark on the window of the page in front of the browser.
const MARK: &str = "window.__pixKept = 'kept'; return 'kept';";

/// The mark on the window, or an empty text once a new document replaced it.
const KEPT: &str = "return window.__pixKept || '';";

/// The id of the top bar's search field.
const SEARCH_FIELD: &str = "pix-search";

/// A synthetic identifier typed into the search.
const TYPED: &str = "SYNTH-4711";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_and_a_search_keep_the_document() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        journey.open(&stack.url("/")).await;
        journey
            .evaluate(MARK)
            .await
            .expect("the window takes a mark");
        journey
            .press(By::Css("a[href='/sources']"), "the sidebar's Sources")
            .await;
        journey
            .address_carrying("/sources", "the sources page")
            .await;
        assert_eq!(
            "kept",
            journey.evaluate(KEPT).await.expect("the window's mark"),
            "a link draws its page in the document the browser holds"
        );
        let field = journey
            .element(By::Id(SEARCH_FIELD), "the top bar's search")
            .await;
        field
            .send_keys(TYPED)
            .await
            .expect("the field takes the text");
        journey
            .keys(&field, Key::Enter, "the search's results")
            .await;
        journey
            .address_carrying("/search", "the results page")
            .await;
        journey
            .count_becoming(
                By::Css(".fui-worklist-grid tr[data-key]"),
                2,
                "two result rows",
            )
            .await;
        assert_eq!(
            "kept",
            journey.evaluate(KEPT).await.expect("the window's mark"),
            "a search draws its results in the document the browser holds"
        );
    })
    .await;
}
