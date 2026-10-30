// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Every screen with FerroUI's script modules withheld: its content is all
//! there, each of its links leads somewhere and each of its forms posts
//! somewhere, no control that works only with a module shows, and at 320 px
//! it reflows with no sideways scroll (WCAG 2.2 SC 1.4.10).
//!
//! The browser is refused every module under
//! `/ferrohealth-ui/<version>/scripts/`, the loader among them, as on a page
//! whose modules have not loaded or will not, which is the floor of
//! FerroUI's interaction contract: complete server-rendered HTML, and a link
//! or a form behind every important action (FerroUI `docs/consoles.md`,
//! "Before you write UI code"; the gallery's `tests/it/no_script.rs`). The
//! walk covers every screen's refused state and its filled state, and a
//! patient search and a reveal, each a plain post. No specification governs
//! this: our own design.

use thirtyfour::prelude::*;

use crate::journeys::accessibility::{REFLOW_WIDTH, SIDEWAYS};
use crate::journeys::browser::{Journey, WINDOW, with_journey_withheld};
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{FILLED_PERSON, Server, Stack};
use crate::journeys::{SCREENS, SIGN_IN_SCREENS, journeys_enabled};

/// What a page shows with no module: one `h1`, no loader mark, every link
/// with an address, every form with an action, and no control that a module
/// shows only once it runs. Each finding is a line; an empty text passes.
const FLOOR: &str = r#"
const findings = [];
if (document.documentElement.hasAttribute('data-fui-script')) {
  findings.push('the loader ran, so the modules were not withheld');
}
if (document.querySelectorAll('h1').length !== 1) {
  findings.push('the page has ' + document.querySelectorAll('h1').length + ' h1');
}
const shown = (el) => {
  const box = el.getBoundingClientRect();
  return el.checkVisibility() && box.width > 0 && box.height > 0;
};
const name = (el) => el.tagName.toLowerCase() + (el.id ? '#' + el.id : '')
  + ' "' + (el.textContent || '').trim().slice(0, 40) + '"';
for (const link of document.querySelectorAll('a')) {
  if (shown(link) && !link.getAttribute('href')) { findings.push('a link with no address: ' + name(link)); }
}
for (const form of document.querySelectorAll('form')) {
  if (form.getAttribute('method') !== 'dialog' && !form.getAttribute('action')) {
    findings.push('a form with no action: ' + name(form));
  }
}
for (const button of document.querySelectorAll('button')) {
  if (!shown(button) || button.disabled) { continue; }
  const submits = (button.getAttribute('type') || 'submit') === 'submit'
    && (button.form !== null);
  const commands = (button.hasAttribute('command') && button.hasAttribute('commandfor'))
    || button.hasAttribute('popovertarget');
  const native = button.closest('summary') !== null;
  if (!submits && !commands && !native) {
    findings.push('a control that works only with a module shows: ' + name(button));
  }
}
return findings.join('\n');
"#;

/// The floor's findings on the open page at `path`.
async fn floor(journey: &Journey, path: &str) -> Vec<String> {
    let found = journey
        .evaluate(FLOOR)
        .await
        .expect("the page reads its markup");
    found
        .lines()
        .map(|line| format!("{path}: {line}"))
        .collect()
}

/// Walks every screen `answers` draws with the modules withheld, at the
/// window's width and at the reflow width, and fails with every finding.
async fn every_screen(answers: Server) {
    let stack = Stack::start_with(answers).await;
    with_journey_withheld(async |journey| {
        let mut findings = Vec::new();
        for path in SIGN_IN_SCREENS {
            journey.open(&stack.url(path)).await;
            findings.extend(floor(journey, path).await);
        }
        sign_in(journey, &stack).await;
        for path in SCREENS {
            journey.open(&stack.url(path)).await;
            findings.extend(floor(journey, path).await);
        }
        journey.resize(REFLOW_WIDTH, WINDOW.1).await;
        for path in SCREENS {
            journey.open(&stack.url(path)).await;
            let over = journey
                .evaluate(SIDEWAYS)
                .await
                .expect("the page reads its width");
            if !over.is_empty() {
                findings.push(format!(
                    "{path} at {REFLOW_WIDTH} px with no module scrolls sideways by {over}"
                ));
            }
        }
        journey.resize(WINDOW.0, WINDOW.1).await;
        assert!(
            findings.is_empty(),
            "with the modules withheld, the {answers:?} screens fall below the floor {} times:\n{}",
            findings.len(),
            findings.join("\n")
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_screens_refused_state_holds_its_floor_with_the_modules_withheld() {
    if !journeys_enabled() {
        return;
    }
    every_screen(Server::Refusing).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_screens_filled_state_holds_its_floor_with_the_modules_withheld() {
    if !journeys_enabled() {
        return;
    }
    every_screen(Server::Filled).await;
}

/// A synthetic identifier typed into the search.
const TYPED: &str = "SYNTH-4711";

/// Whether the page's main content draws `words`, as `yes` or `no`.
const DRAWS: &str =
    "return document.querySelector('main').innerText.includes(arguments[0]) ? 'yes' : 'no';";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_patient_search_and_a_reveal_post_with_the_modules_withheld() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start_with(Server::Filled).await;
    with_journey_withheld(async |journey| {
        sign_in(journey, &stack).await;
        journey.open(&stack.url("/search")).await;
        let field = journey
            .element(By::Id("identifier"), "the masked identifier field")
            .await;
        assert_eq!(
            Some(String::from("password")),
            field.attr("type").await.expect("the field's type"),
            "the identifier is masked as it is typed"
        );
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
        journey
            .open(&stack.url(&format!("/patients/{FILLED_PERSON}")))
            .await;
        let reason = journey
            .element(By::Id("reason-0"), "the first reveal's reason")
            .await;
        reason
            .send_keys("Synthetic ward call")
            .await
            .expect("the reason");
        journey.keys(&reason, Key::Enter, "the reveal").await;
        assert_eq!(
            "yes",
            journey
                .evaluate_with(DRAWS, String::from(TYPED))
                .await
                .expect("the page reads its words"),
            "the revealed value is drawn in place"
        );
        let address = journey
            .address_carrying("/patients/", "the patient page")
            .await;
        assert!(
            !address.contains(TYPED),
            "the revealed value stays out of the address: {address}"
        );
    })
    .await;
}
