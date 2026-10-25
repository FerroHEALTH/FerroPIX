// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The keyboard paths of the console, against the filled stand-in: `/`
//! focuses the top bar's search from anywhere, `j` and `k` walk the search
//! results and Enter opens the person, a reveal shows the value with its
//! countdown and the trail lists the disclosure, and a modal dialog gives
//! the focus back to the control that opened it when Escape closes it (WCAG 2.2 SC
//! 2.1.1 and SC 2.4.3; WAI-ARIA Authoring Practices, the grid and the dialog
//! (modal) patterns). No specification governs the keys themselves: our own
//! design, on FerroUI's search box and work-list.

// TODO(#204): prove `?` opens the shortcut legend and `1` to `4` switch the
// patient tabs, once FerroUI carries both.

use thirtyfour::prelude::*;

use crate::journeys::browser::with_journey;
use crate::journeys::journeys_enabled;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{FILLED_PERSON, Server, Stack};

/// The id of the top bar's search field.
const SEARCH_FIELD: &str = "pix-search";

/// A synthetic identifier typed into the search.
const TYPED: &str = "SYNTH-4711";

/// The synthetic value the filled stand-in reveals.
const REVEALED: &str = "SYNTH-4711";

/// The id of the active element, or an empty text.
const ACTIVE_ID: &str = "return document.activeElement ? document.activeElement.id : '';";

/// The `data-key` of the result row that holds the focus, or an empty text.
const FOCUSED_ROW: &str = "const row = document.activeElement; return row && row.matches('tr[data-key]') ? row.dataset.key : '';";

/// Puts the focus on the first result row.
const FIRST_ROW: &str = "const row = document.querySelector('.fui-worklist-grid tr[data-key]'); row.focus(); return row.dataset.key;";

/// The `command` and `commandfor` of the active element, or an empty text.
const ACTIVE_COMMAND: &str = "const el = document.activeElement; return el ? (el.getAttribute('command') || '') + ' ' + (el.getAttribute('commandfor') || '') : '';";

/// Whether the dialog of the preferences' reset is open.
const RESET_OPEN: &str = "return String(document.getElementById('preferences-reset').open);";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slash_focuses_the_search_and_j_k_and_enter_walk_and_open_the_results() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        journey.open(&stack.url("/audit")).await;
        journey
            .element(By::Tag("body"), "the page's body")
            .await
            .send_keys("/")
            .await
            .expect("the page takes the slash");
        assert_eq!(
            SEARCH_FIELD,
            journey
                .evaluate(ACTIVE_ID)
                .await
                .expect("the active element"),
            "the slash focuses the top bar's search"
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
        let first = journey.evaluate(FIRST_ROW).await.expect("the first row");
        assert_eq!(FILLED_PERSON, first);
        let row = journey.active().await;
        row.send_keys("j").await.expect("the row takes j");
        let second = journey
            .evaluate(FOCUSED_ROW)
            .await
            .expect("the focused row");
        assert_ne!(first, second, "j moves to the next row");
        assert!(!second.is_empty(), "the focus stays on a row");
        let row = journey.active().await;
        row.send_keys("k").await.expect("the row takes k");
        assert_eq!(
            first,
            journey
                .evaluate(FOCUSED_ROW)
                .await
                .expect("the focused row"),
            "k moves back"
        );
        let row = journey.active().await;
        journey.keys(&row, Key::Enter, "the person's page").await;
        journey
            .address_carrying(
                &format!("/patients/{FILLED_PERSON}"),
                "Enter to open the person",
            )
            .await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dialog_closed_with_escape_gives_the_focus_back_to_its_button() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        journey.open(&stack.url("/preferences")).await;
        let opener = journey
            .element(
                By::Css("button[commandfor='preferences-reset']"),
                "the reset's button",
            )
            .await;
        opener.focus().await.expect("the button takes the focus");
        opener
            .send_keys(Key::Enter)
            .await
            .expect("the button takes Enter");
        assert_eq!(
            "true",
            journey
                .evaluate(RESET_OPEN)
                .await
                .expect("the dialog's state"),
            "Enter on the button opens the dialog"
        );
        journey
            .active()
            .await
            .send_keys(Key::Escape)
            .await
            .expect("the dialog takes Escape");
        assert_eq!(
            "false",
            journey
                .evaluate(RESET_OPEN)
                .await
                .expect("the dialog's state"),
            "Escape closes the dialog"
        );
        assert_eq!(
            "show-modal preferences-reset",
            journey
                .evaluate(ACTIVE_COMMAND)
                .await
                .expect("the active element"),
            "the focus is back on the button that opened the dialog"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reveal_shows_the_value_with_its_countdown_and_the_trail_shows_the_disclosure() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        journey
            .open(&stack.url(&format!("/patients/{FILLED_PERSON}")))
            .await;
        let reason = journey
            .element(By::Id("reason-0"), "the reveal's reason")
            .await;
        reason
            .send_keys("Synthetic check")
            .await
            .expect("the field takes the reason");
        journey
            .keys(&reason, Key::Enter, "the revealed value")
            .await;
        let address = journey
            .address_carrying(&format!("/patients/{FILLED_PERSON}"), "the record again")
            .await;
        assert!(
            !address.contains(REVEALED),
            "the value stays out of the address"
        );
        journey.element(By::Css("code"), "the revealed value").await;
        let page = journey
            .evaluate("return document.body.innerText;")
            .await
            .expect("the page's text");
        assert!(page.contains(REVEALED), "the value is shown");
        journey
            .element(By::Css("[role='timer']"), "the countdown")
            .await;
        journey
            .element(
                By::Css("[role='status']"),
                "the countdown's live announcement",
            )
            .await;
        journey.open(&stack.url("/audit")).await;
        let trail = journey
            .evaluate("return document.body.innerText;")
            .await
            .expect("the trail's text");
        assert!(trail.contains("Reveal"), "the trail lists the disclosure");
        assert!(!trail.contains(REVEALED), "the trail never shows the value");
    })
    .await;
}
