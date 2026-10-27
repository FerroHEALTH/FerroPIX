// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The end of a session in a real browser, on FerroUI's session watch
//! (FerroUI `docs/consoles.md`, "The session's end"): signing out in one tab
//! ends the other, which loads the sign-in with its own address as `return`
//! and comes back to it after the next sign-in, and the warning before the
//! idle timeout keeps the session when the steward stays signed in (WCAG 2.2
//! SC 2.2.1). No specification governs the session store: our own design.

use thirtyfour::prelude::*;

use crate::journeys::browser::{Journey, with_journey};
use crate::journeys::journeys_enabled;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::Stack;

/// The idle timeout of the warning's console, in seconds: ten seconds more
/// than the warning FerroUI opens before it, so it opens ten seconds after
/// a page is drawn.
const IDLE: u64 = 130;

/// The id of FerroUI's warning dialog (`widget::session::WARNING_ID`).
const WARNING: &str = "ferroui-session-warning";

/// Reads the console's session route from the page, as the watch does,
/// and answers the seconds left until the idle timeout.
const IDLE_LEFT: &str = r"
const done = arguments[arguments.length - 1];
fetch('/session', { credentials: 'same-origin' })
  .then((response) => response.json())
  .then((left) => done(String(left.idle)), (error) => done(`unread: ${error}`));
";

/// The seconds the session has left before its idle timeout.
async fn idle_left(journey: &Journey) -> u64 {
    let answer: String = journey
        .driver()
        .execute_async(IDLE_LEFT, Vec::new())
        .await
        .expect("the page reads the session route")
        .convert()
        .expect("the answer is text");
    answer
        .parse()
        .unwrap_or_else(|_| panic!("the session route answered {answer:?}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signing_out_in_one_tab_sends_the_other_to_sign_in_and_back() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start().await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        journey.open(&stack.url("/sources")).await;
        let watched = journey
            .driver()
            .window()
            .await
            .expect("the first tab's handle");
        let other = journey
            .driver()
            .new_tab()
            .await
            .expect("a second tab opens");
        journey
            .driver()
            .switch_to_window(other)
            .await
            .expect("the second tab takes the focus");
        journey.open(&stack.url("/audit")).await;
        journey
            .press(By::Css("button.fui-operator"), "the user menu's button")
            .await;
        journey
            .press(
                By::Css("form[action='/logout'] button[type='submit']"),
                "Sign out in the user menu",
            )
            .await;
        journey
            .address_carrying("/sign-in/signed-out", "the signed-out page")
            .await;
        journey
            .driver()
            .switch_to_window(watched)
            .await
            .expect("the first tab takes the focus again");
        journey
            .address_carrying(
                "/sign-in/expired?return=/sources",
                "the first tab to load the sign-in with its own address as return",
            )
            .await;
        journey
            .press(
                By::Css("a[href='/login?return=/sources']"),
                "the provider's button carrying the return",
            )
            .await;
        journey
            .address_carrying("/sources", "the sign-in to lead back to the sources")
            .await;
        journey
            .element(
                By::Css("button.fui-operator"),
                "the shell of a signed-in page",
            )
            .await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stay_signed_in_in_the_idle_warning_extends_the_session() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_idle(IDLE).await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        let stay = journey
            .element(
                By::Css(format!("#{WARNING}[open] button.fui-button-primary")),
                "the warning before the idle timeout, with Stay signed in",
            )
            .await;
        let before = idle_left(journey).await;
        assert!(
            before <= IDLE - 10,
            "the warning opens with {before} seconds left"
        );
        journey.press_on(&stay, "Stay signed in").await;
        journey
            .count_becoming(
                By::Css(format!("#{WARNING}[open]")),
                0,
                "the warning to close",
            )
            .await;
        let after = idle_left(journey).await;
        assert!(
            after > before && after >= IDLE - 5,
            "Stay signed in restarts the idle timeout: {before} seconds left before, {after} after"
        );
    })
    .await;
}
