// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! How the console moves between pages: a link and a patient search load
//! one whole document the server drew, and an in-page change, a row's side
//! panel opened and closed, loads none and keeps the page where it was.
//! A mark set on the window before the press survives it only when the
//! browser loaded no new document (the HTML Living Standard, "Navigating
//! across documents", gives each loaded document a new `Window`). No
//! specification governs the console's navigation: our own design, on
//! FerroUI's server-rendered pages (FerroUI `docs/migration-0.6.md`, "Tests";
//! the gallery's `tests/it/navigation.rs`).

use thirtyfour::prelude::*;

use crate::journeys::browser::with_journey;
use crate::journeys::journeys_enabled;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{FILLED_CANDIDATE, Server, Stack};

/// Sets the mark on the window of the page in front of the browser.
const MARK: &str = "window.__pixKept = 'kept'; return 'kept';";

/// The mark on the window, or an empty text once a new document replaced it.
const KEPT: &str = "return window.__pixKept || '';";

/// Whether the dialog `arguments[0]` names is open, as `true` or `false`.
const OPEN: &str = "return String(!!document.getElementById(arguments[0])?.open);";

/// The id of the top bar's search field.
const SEARCH_FIELD: &str = "pix-search";

/// A synthetic identifier typed into the search.
const TYPED: &str = "SYNTH-4711";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_and_a_search_load_a_document_and_a_side_panel_loads_none() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        journey.open(&stack.url("/queue")).await;
        journey
            .evaluate(MARK)
            .await
            .expect("the window takes a mark");
        let panel = format!("glance-{FILLED_CANDIDATE}");
        journey
            .press(
                By::Css(format!("button[commandfor='{panel}']")),
                "the first row's side panel",
            )
            .await;
        assert_eq!(
            "true",
            journey
                .evaluate_with(OPEN, panel.clone())
                .await
                .expect("the panel's state"),
            "the side panel opens over the queue"
        );
        journey
            .press(
                By::Css(format!("#{panel} button[command='close']")),
                "the side panel's Close",
            )
            .await;
        assert_eq!(
            "false",
            journey
                .evaluate_with(OPEN, panel)
                .await
                .expect("the panel's state"),
            "the side panel closes"
        );
        assert_eq!(
            "kept",
            journey.evaluate(KEPT).await.expect("the window's mark"),
            "a side panel opens and closes in the document the browser holds"
        );
        journey
            .press(By::Css("a[href='/sources']"), "the sidebar's Sources")
            .await;
        journey
            .address_carrying("/sources", "the sources page")
            .await;
        assert_eq!(
            "",
            journey.evaluate(KEPT).await.expect("the window's mark"),
            "a link loads the page the server drew as a document of its own"
        );
        journey
            .evaluate(MARK)
            .await
            .expect("the window takes a mark");
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
        let address = journey
            .address_carrying("/search", "the results page")
            .await;
        assert!(
            !address.contains(TYPED),
            "the identifier stays out of the address: {address}"
        );
        journey
            .count_becoming(
                By::Css(".fui-worklist-grid tr[data-key]"),
                2,
                "two result rows",
            )
            .await;
        assert_eq!(
            "",
            journey.evaluate(KEPT).await.expect("the window's mark"),
            "a search posts and loads the results the server drew"
        );
    })
    .await;
}
