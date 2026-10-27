// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pseudo-locale journey (#97): every screen drawn in the console's
//! pseudo-locale (`qps`), where every console and library string is accented
//! and two fifths longer.
//!
//! A word a reader sees that is not stretched never went through a catalog,
//! which is the stray literal the static check of
//! `scripts/checks/stray-literals.sh` cannot see from the source (a word
//! built at run time, a library word with no message). A screen that scrolls
//! sideways at 1440 or 390 px leaves no room for a longer translation. Text
//! that is data or code, not words, is passed over: what sits in `code`,
//! `pre`, `kbd`, `samp` or `time`, a number, an address, an identifier.
//! No specification governs pseudo-localization: our own design, after
//! FerroUI's gallery.

use crate::journeys::browser::{Journey, WINDOW, with_journey};
use crate::journeys::journeys_enabled;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{PREFERENCE_PREFIX, STEWARD, Server, Stack};
use crate::journeys::{SCREENS, SIGN_IN_SCREENS};

/// The pseudo-locale's tag.
const PSEUDO: &str = "qps";

/// The widths the layout is held at: the design's desktop and phone frames.
const WIDTHS: [u32; 2] = [1440, 390];

/// Every visible text on the page that the pseudo-locale did not stretch,
/// one per line.
///
/// A message is drawn stretched, a bracketed run ending in tildes, and may
/// sit beside data in one text node (a version, an address) or hold another
/// message in a placeable. So the stretched runs, innermost first, and the
/// names [`KEPT`] lists are taken out, and what is
/// left must hold no word: a token of letters alone. A token with a digit,
/// a dot, a slash or a hyphen in it is data (`0.0.2`, `ferrohealth-ui`).
const UNSTRETCHED: &str = r#"
const kept = JSON.parse(arguments[0]);
const passed = (el) => el.closest('code, pre, kbd, samp, time, script, style, svg, [translate="no"]');
const left = (text) => {
  let rest = text;
  for (let before = ''; before !== rest;) { before = rest; rest = rest.replace(/\[[^\[\]]*~+\]/g, ' '); }
  for (const name of kept) { rest = rest.split(name).join(' '); }
  return rest.split(/[\s·|,;:()]+/).filter((token) => /^[A-Za-z\u00C0-\u024F]{2,}[.!?]?$/.test(token));
};
const found = new Set();
const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
for (let node = walker.nextNode(); node; node = walker.nextNode()) {
  const text = node.textContent.trim();
  if (text === '') { continue; }
  const el = node.parentElement;
  if (!el || passed(el)) { continue; }
  const hidden = !el.checkVisibility({ visibilityProperty: true, opacityProperty: true });
  if (hidden && !el.closest('.fui-sr-only, .sr-only')) { continue; }
  if (left(text).length > 0) { found.add(el.tagName.toLowerCase() + ': ' + text.slice(0, 60)); }
}
for (const el of document.querySelectorAll('[aria-label], [title], [placeholder], [alt]')) {
  if (passed(el)) { continue; }
  for (const name of ['aria-label', 'title', 'placeholder', 'alt']) {
    const value = (el.getAttribute(name) || '').trim();
    if (left(value).length > 0) { found.add(el.tagName.toLowerCase() + '[' + name + ']: ' + value.slice(0, 60)); }
  }
}
return Array.from(found).join('\n');
"#;

/// The words that are the same in every language and so are never
/// stretched: the maker's legal name, the steward's name, which the
/// provider sent and the console draws as it came, and the data the filled
/// stand-in serves, drawn as the server sent it.
const KEPT: [&str; 12] = [
    "Cadasto B.V.",
    STEWARD,
    "Ada Proefsma",
    "Synthetic hospital",
    "Synthetic Steward",
    "Synthetic check",
    "Synthetic clinical domain",
    "Synthetic research domain",
    "Synthetic phone call",
    "EAST|ADT",
    "AA",
    "AE",
];

/// Whether the page scrolls sideways: the overflow in CSS pixels and the
/// innermost elements that reach past the viewport, or an empty string when
/// it fits.
const SIDEWAYS: &str = r"
const width = document.documentElement.clientWidth;
const over = document.documentElement.scrollWidth - width;
if (over <= 1) { return ''; }
const past = Array.from(document.querySelectorAll('body *')).filter((el) => {
  const box = el.getBoundingClientRect();
  return box.width > 0 && box.right > width + 1
    && !Array.from(el.children).some((child) => child.getBoundingClientRect().right > width + 1);
});
const named = past.slice(0, 4).map((el) => el.tagName.toLowerCase()
  + (el.className && typeof el.className === 'string' ? '.' + el.className.trim().split(/\s+/).join('.') : '')
  + ' (' + Math.round(el.getBoundingClientRect().right) + ' px)');
return String(over) + ' px, past the edge: ' + named.join(', ');
";

/// Opens `path` and returns what the pseudo-locale left unstretched on it
/// and whether it scrolls sideways at `width`.
async fn read(journey: &Journey, stack: &Stack, path: &str, width: u32) -> Vec<String> {
    journey.open(&stack.url(path)).await;
    let mut findings = Vec::new();
    let lang = journey
        .evaluate("return document.documentElement.lang;")
        .await
        .expect("the page reads its language");
    if lang != PSEUDO {
        findings.push(format!("{path}: <html lang> is {lang:?}, not {PSEUDO}"));
    }
    let kept = serde_json::to_string(&KEPT).expect("the kept names as JSON");
    let left = journey
        .evaluate_with(UNSTRETCHED, kept)
        .await
        .expect("the page reads its words");
    if width == WIDTHS[0] {
        findings.extend(
            left.lines()
                .map(|line| format!("{path}: unstretched {line}")),
        );
    }
    let over = journey
        .evaluate(SIDEWAYS)
        .await
        .expect("the page reads its width");
    if !over.is_empty() {
        findings.push(format!("{path} at {width} px scrolls sideways by {over}"));
    }
    findings
}

/// Reads every screen `answers` draws in the pseudo-locale at both widths,
/// and fails with every defect.
async fn every_screen(answers: Server) {
    let stack = Stack::start_with(answers).await;
    with_journey(async |journey| {
        journey.open(&stack.url("/sign-in")).await;
        journey
            .set_cookie(&format!("{PREFERENCE_PREFIX}language"), PSEUDO)
            .await
            .expect("the browser takes the language cookie");
        let mut findings = Vec::new();
        for width in WIDTHS {
            journey.resize(width, WINDOW.1).await;
            for path in SIGN_IN_SCREENS {
                findings.extend(read(journey, &stack, path, width).await);
            }
        }
        journey.resize(WINDOW.0, WINDOW.1).await;
        sign_in(journey, &stack).await;
        for width in WIDTHS {
            journey.resize(width, WINDOW.1).await;
            for path in SCREENS {
                findings.extend(read(journey, &stack, path, width).await);
            }
        }
        assert!(
            findings.is_empty(),
            "the pseudo-locale over the {answers:?} screens found {} defects:\n{}",
            findings.len(),
            findings.join("\n")
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_screen_in_the_pseudo_locale_stretches_every_word_and_keeps_its_layout() {
    if !journeys_enabled() {
        return;
    }
    every_screen(Server::Refusing).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_filled_screen_in_the_pseudo_locale_stretches_every_word_and_keeps_its_layout() {
    if !journeys_enabled() {
        return;
    }
    every_screen(Server::Filled).await;
}
