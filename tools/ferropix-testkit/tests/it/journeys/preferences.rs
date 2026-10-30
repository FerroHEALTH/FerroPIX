// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Your preferences in a real browser: a choice applies on the change once
//! FerroUI's modules run, is kept in its own cookie, is on `<html>` at the
//! first paint of the next page, and survives a fresh load
//! (`.claude/rules/console-ui.md` §4, FerroUI family rule 9).

use std::time::Instant;

use thirtyfour::prelude::*;

use crate::journeys::browser::{Journey, WAIT, with_journey};
use crate::journeys::journeys_enabled;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{PREFERENCE_PREFIX, Stack};

/// Chooses the radio card `value` of the preference `name` on Appearance,
/// and waits for the console to draw the page again with `drawn` on
/// `<html>` carrying it.
pub(crate) async fn choose(journey: &Journey, stack: &Stack, name: &str, value: &str, drawn: &str) {
    journey.open(&stack.url("/preferences")).await;
    enhanced(journey, name).await;
    let kept = By::Css(format!("html[{drawn}='{value}']"));
    let card = journey
        .element(
            By::Css(format!("label:has(input[name='{name}'][value='{value}'])")),
            &format!("the {name} choice {value}"),
        )
        .await;
    journey
        .press_on(&card, &format!("the {name} choice {value}"))
        .await;
    journey
        .count_becoming(kept, 1, &format!("<html> to carry the {name} {value}"))
        .await;
}

/// Waits until FerroUI's `preference` module has taken the form of the
/// preference `name`: it hides the form's Save, which a change no longer
/// needs, since it posts the form at once.
async fn enhanced(journey: &Journey, name: &str) {
    journey
        .count_becoming(
            By::Css(format!(
                "form:has([name='{name}']) button[type='submit'][hidden]"
            )),
            1,
            &format!("the {name} form to post on a change"),
        )
        .await;
}

/// Waits until the browser holds the preference cookie `name` at `value`.
async fn cookie_becoming(journey: &Journey, name: &str, value: &str) {
    let cookie = format!("{PREFERENCE_PREFIX}{name}");
    let started = Instant::now();
    loop {
        let held = journey
            .cookie(&cookie)
            .await
            .expect("the browser reads its cookies")
            .map(|cookie| cookie.value);
        if held.as_deref() == Some(value) {
            return;
        }
        assert!(
            started.elapsed() <= WAIT,
            "the cookie {cookie} holds {held:?}, never {value}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_preference_round_trips_through_its_cookie_to_the_first_paint() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start().await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;

        choose(journey, &stack, "theme", "dark", "data-theme").await;
        cookie_becoming(journey, "theme", "dark").await;
        choose(journey, &stack, "density", "compact", "data-density").await;
        cookie_becoming(journey, "density", "compact").await;

        enhanced(journey, "reveal").await;
        let option = journey
            .element(
                By::Css("select[name='reveal'] option[value='30']"),
                "the reveal duration of 30 seconds",
            )
            .await;
        journey
            .press_on(&option, "the reveal duration of 30 seconds")
            .await;
        cookie_becoming(journey, "reveal", "30").await;

        journey.open(&stack.url("/audit")).await;
        journey
            .count_becoming(
                By::Css("html[data-theme='dark'][data-density='compact']"),
                1,
                "the next page to be drawn dark and compact from its first paint",
            )
            .await;
        journey.open(&stack.url("/preferences")).await;
        journey
            .count_becoming(
                By::Css("select[name='reveal'] option[value='30']:checked"),
                1,
                "the reveal duration to read back as 30 seconds",
            )
            .await;

        choose(journey, &stack, "theme", "light", "data-theme").await;
        cookie_becoming(journey, "theme", "light").await;
    })
    .await;
}
