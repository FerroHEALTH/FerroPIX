// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The WCAG 2.2 Level AA battery over every screen, walked in a real browser
//! over what it painted (<https://www.w3.org/TR/WCAG22/>).
//!
//! The battery runs twice: over every screen's refused state, against a
//! stand-in that serves no operator API, and over its filled state, against
//! one that serves the synthetic documents. Per screen, in the light and
//! the dark theme: every text meets its contrast bar (SC 1.4.3), every
//! control's edge or fill meets 3:1 against what is behind it (SC 1.4.11),
//! the keyboard reaches every control and every stop draws a focus ring
//! (SC 2.1.1, SC 2.4.7), the markup names every control once and the page
//! has one `h1` (SC 1.3.1, SC 4.1.2), and at 320 px the page reflows with no
//! sideways scroll (SC 1.4.10). A self-test paints a deliberately wrong
//! theme and proves the colour checks refuse it.

use std::time::Duration;

use crate::journeys::browser::{Journey, WINDOW, with_journey};
use crate::journeys::journeys_enabled;
use crate::journeys::preferences::choose;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{Server, Stack};
use crate::journeys::{SCREENS, SIGN_IN_SCREENS};

/// The two themes a steward can choose between.
const THEMES: [&str; 2] = ["light", "dark"];

/// The width SC 1.4.10 names: 320 CSS pixels.
const REFLOW_WIDTH: u32 = 320;

/// How many tab presses past the number of controls a walk allows itself:
/// the document and the browser's own chrome take stops on the way round.
const EXTRA_STOPS: usize = 6;

/// How long the count of controls has to hold still before a walk trusts
/// it, and how many times it is read.
const SETTLE: Duration = Duration::from_millis(300);

/// How many times a walk reads the count of controls before it gives up.
const SETTLE_TRIES: usize = 20;

/// The colour arithmetic every colour check shares: a computed colour read
/// in any space a browser writes (rgb, oklch, oklab, `color(srgb …)`),
/// composited over what is behind it, and WCAG's contrast ratio over the
/// relative luminance (<https://www.w3.org/TR/WCAG22/#dfn-contrast-ratio>).
/// FerroTERM's battery, whole.
const COLOUR: &str = r"
const linear = (value) => {
  const c = value / 255;
  return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
};
const gamma = (c) => {
  const v = c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(Math.max(c, 0), 1 / 2.4) - 0.055;
  return Math.round(Math.min(Math.max(v, 0), 1) * 255);
};
const from_oklab = (L, a, b, alpha) => {
  const l = Math.pow(L + 0.3963377774 * a + 0.2158037573 * b, 3);
  const m = Math.pow(L - 0.1055613458 * a - 0.0638541728 * b, 3);
  const s = Math.pow(L - 0.0894841775 * a - 1.2914855480 * b, 3);
  return {
    r: 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    g: -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    b: -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    a: alpha,
  };
};
const numbers = (colour) => (colour.match(/-?[\d.]+(?:e-?\d+)?%?/g) || []).map((part) =>
  part.endsWith('%') ? Number(part.slice(0, -1)) / 100 : Number(part)
);
const parse = (colour) => {
  const text = colour.trim();
  if (text === 'transparent') { return { r: 0, g: 0, b: 0, a: 0 }; }
  const parts = numbers(text);
  if (parts.length < 3) { return null; }
  const alpha = parts.length > 3 ? parts[3] : 1;
  if (text.startsWith('rgb')) {
    return { r: linear(parts[0]), g: linear(parts[1]), b: linear(parts[2]), a: alpha };
  }
  if (text.startsWith('oklch')) {
    const hue = (parts[2] * Math.PI) / 180;
    return from_oklab(parts[0], parts[1] * Math.cos(hue), parts[1] * Math.sin(hue), alpha);
  }
  if (text.startsWith('color(srgb ')) {
    return { r: linear(parts[0] * 255), g: linear(parts[1] * 255), b: linear(parts[2] * 255), a: alpha };
  }
  if (text.startsWith('oklab')) { return from_oklab(parts[0], parts[1], parts[2], alpha); }
  return null;
};
const encoded = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(Math.max(c, 0), 1 / 2.4) - 0.055);
const blend = (top, bottom, alpha) => linear(255 * (encoded(top) * alpha + encoded(bottom) * (1 - alpha)));
const over = (top, bottom) => ({
  r: blend(top.r, bottom.r, top.a), g: blend(top.g, bottom.g, top.a), b: blend(top.b, bottom.b, top.a), a: 1,
});
const luminance = (c) => 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
const ratio = (one, two) => {
  const lighter = Math.max(luminance(one), luminance(two));
  const darker = Math.min(luminance(one), luminance(two));
  return (lighter + 0.05) / (darker + 0.05);
};
const shown = (c) => 'rgb(' + gamma(c.r) + ', ' + gamma(c.g) + ', ' + gamma(c.b) + ')';
const unknown = new Set();
const behind = (el) => {
  let painted = { r: 1, g: 1, b: 1, a: 1 };
  const stack = [];
  for (let node = el; node; node = node.parentElement) {
    const raw = getComputedStyle(node).backgroundColor;
    const colour = parse(raw);
    if (!colour) { unknown.add(raw); continue; }
    if (colour.a === 0) { continue; }
    stack.push(colour);
    if (colour.a === 1) { break; }
  }
  while (stack.length) { painted = over(stack.pop(), painted); }
  return painted;
};
const drawn = (el) => {
  const box = el.getBoundingClientRect();
  return box.width >= 2 && box.height >= 2
    && el.checkVisibility({ visibilityProperty: true, opacityProperty: true });
};
const label = (el) => el.tagName.toLowerCase() + (el.id ? '#' + el.id : '');
";

/// Every text whose painted contrast is under its bar: 4.5:1, or 3:1 for
/// large text, 24px or 18.66px bold (SC 1.4.3).
const CONTRAST: &str = r#"
const failures = [];
const owns_text = (el) => Array.from(el.childNodes).some(
  (node) => node.nodeType === Node.TEXT_NODE && node.textContent.trim() !== ''
);
for (const el of document.querySelectorAll('body *')) {
  if (!owns_text(el) || !drawn(el)) { continue; }
  if (el.closest('.sr-only, .fui-sr-only') || el.tagName === 'OPTION') { continue; }
  const style = getComputedStyle(el);
  const fg = parse(style.color);
  if (!fg) { unknown.add(style.color); continue; }
  if (fg.a === 0) { continue; }
  const bg = behind(el);
  const painted = fg.a === 1 ? fg : over(fg, bg);
  const size = parseFloat(style.fontSize);
  const weight = Number(style.fontWeight) || 400;
  const bar = size >= 24 || (size >= 18.66 && weight >= 700) ? 3 : 4.5;
  const measured = ratio(painted, bg);
  if (measured + 0.005 < bar) {
    failures.push('text ' + label(el) + ' "' + el.textContent.trim().slice(0, 40) + '" '
      + measured.toFixed(2) + ':1 needs ' + bar + ':1, ' + shown(painted) + ' on ' + shown(bg));
  }
}
for (const raw of unknown) { failures.push('unreadable colour: ' + raw); }
return failures.join('\n');
"#;

/// Every form control whose edge and fill both fall under 3:1 against what
/// is behind it, so nothing shows where it is (SC 1.4.11). A library field
/// draws its box on a wrapper around a bare input, so the control, or an
/// ancestor up to two levels that is about its size, may be the box. A
/// checkbox or radio the browser draws itself (`appearance` not `none`)
/// shows the edge of the browser's own theme and is not measured.
const EDGES: &str = r"
const failures = [];
const outside_of = (el) => (el.parentElement ? behind(el.parentElement) : { r: 1, g: 1, b: 1, a: 1 });
const shows = (box) => {
  const style = getComputedStyle(box);
  const outside = outside_of(box);
  const width = parseFloat(style.borderBottomWidth) || 0;
  const raw = style.borderBottomColor;
  const border = parse(raw);
  if (!border) { unknown.add(raw); return { best: 0, outside }; }
  const edge = width >= 1 && border.a > 0 ? ratio(border.a === 1 ? border : over(border, outside), outside) : 1;
  return { best: Math.max(edge, ratio(behind(box), outside)), outside };
};
for (const el of document.querySelectorAll('input, select, textarea')) {
  if (el.type === 'hidden' || !drawn(el)) { continue; }
  if (el.closest('.sr-only, .fui-sr-only')) { continue; }
  if ((el.type === 'checkbox' || el.type === 'radio') && getComputedStyle(el).appearance !== 'none') { continue; }
  const own = el.getBoundingClientRect();
  let best = 0;
  let outside = outside_of(el);
  for (let box = el, depth = 0; box && depth < 3; box = box.parentElement, depth += 1) {
    const rect = box.getBoundingClientRect();
    if (depth > 0 && (rect.height > own.height + 24 || rect.width > own.width * 1.5 + 32)) { break; }
    const found = shows(box);
    if (found.best > best) { best = found.best; outside = found.outside; }
  }
  if (best + 0.005 < 3) {
    failures.push('control ' + label(el) + ' shows at ' + best.toFixed(2) + ':1, need 3:1, on ' + shown(outside));
  }
}
for (const raw of unknown) { failures.push('unreadable colour: ' + raw); }
return failures.join('\n');
";

/// How many visible controls take a stop in the tab order right now.
const COUNT_FOCUSABLE: &str = r"
return String(Array.from(document.querySelectorAll(
  'a[href], button:enabled, input:enabled, select:enabled, textarea:enabled, summary, [tabindex]'
)).filter((el) => el.getAttribute('tabindex') !== '-1' && !(el.type === 'hidden')).filter((el) => {
  const box = el.getBoundingClientRect();
  return box.width >= 2 && box.height >= 2
    && el.checkVisibility({ visibilityProperty: true, opacityProperty: true });
}).length);
";

/// Tags every visible control that takes a tab stop, starts recording each
/// stop focus reaches with the outline it draws there, and lists the tagged
/// controls (FerroTERM's walk). The ring is read where `:focus-visible`
/// matches, on the control or the nearest part that draws it.
const TAG_FOCUSABLE: &str = r#"
const focusable = Array.from(document.querySelectorAll(
  'a[href], button:enabled, input:enabled, select:enabled, textarea:enabled, summary, [tabindex]'
)).filter((el) => el.getAttribute('tabindex') !== '-1' && el.type !== 'hidden').filter((el) => {
  const box = el.getBoundingClientRect();
  if (box.width < 2 || box.height < 2) { return false; }
  return el.checkVisibility({ visibilityProperty: true, opacityProperty: true });
});
focusable.forEach((el, index) => el.setAttribute('data-walk', String(index)));
window.walkStops = [];
document.addEventListener('focusin', (event) => {
  const el = event.target;
  if (!el.matches(':focus-visible')) { return; }
  const draws = (node) => {
    const style = getComputedStyle(node);
    return (style.outlineStyle !== 'none' && !style.outlineWidth.startsWith('0'))
      || style.boxShadow !== 'none';
  };
  let ring = el;
  for (let node = el.parentElement, depth = 0; node && depth < 3 && !draws(ring); node = node.parentElement, depth += 1) {
    if (node.matches(':focus-within') && draws(node)) { ring = node; }
  }
  const style = getComputedStyle(ring);
  const tag = el.getAttribute('data-walk');
  const shape = style.outlineStyle !== 'none' ? style.outlineStyle : (style.boxShadow !== 'none' ? 'shadow' : 'none');
  window.walkStops.push([tag === null ? '' : tag, shape, style.outlineWidth,
    el.tagName.toLowerCase() + (el.id ? '#' + el.id : '')].join('|'));
}, true);
document.body.setAttribute('tabindex', '-1');
document.body.focus();
return focusable.map((el) => el.tagName.toLowerCase() + (el.id ? '#' + el.id : '')
  + ' "' + (el.getAttribute('aria-label') || el.textContent || '').trim().slice(0, 30) + '"').join('\n');
"#;

/// Every stop the walk reached, one per line, as the recorder wrote them.
const WALK_STOPS: &str = "return (window.walkStops || []).join('\\n');";

/// What a screen reader needs of the markup and no wait proves: a name for
/// every control, every id once, one `h1`, an `alt` on every image.
const SEMANTICS: &str = r#"
const findings = [];
const name = (el) => el.tagName.toLowerCase() + (el.id ? '#' + el.id : '');
for (const control of document.querySelectorAll('input, select, textarea')) {
  if (control.type === 'hidden') { continue; }
  const labelled = control.id && document.querySelector('label[for="' + CSS.escape(control.id) + '"]');
  const named = labelled || control.closest('label')
    || control.getAttribute('aria-label') || control.getAttribute('aria-labelledby');
  if (!named) { findings.push('control with no label: ' + name(control)); }
}
for (const button of document.querySelectorAll('button, a[href]')) {
  const named = (button.textContent || '').trim() || button.getAttribute('aria-label')
    || button.getAttribute('aria-labelledby') || button.getAttribute('title');
  if (!named) { findings.push('control with no name: ' + name(button)); }
}
const seen = new Set();
for (const el of document.querySelectorAll('[id]')) {
  if (seen.has(el.id)) { findings.push('id used twice: ' + el.id); }
  seen.add(el.id);
}
const headings = document.querySelectorAll('h1');
if (headings.length !== 1) { findings.push(headings.length + ' h1 elements, not 1'); }
for (const image of document.querySelectorAll('img')) {
  if (image.getAttribute('alt') === null) { findings.push('img with no alt: ' + image.src); }
}
if (!document.documentElement.lang) { findings.push('<html> names no language'); }
return findings.join('\n');
"#;

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

/// A deliberately wrong theme for the self-test: grey words on white, and a
/// field with a pale edge on white, set through the CSSOM, which the
/// console's Content-Security-Policy leaves open to a script where it
/// refuses an inline style sheet. The field is one of its own, since the
/// library's field styles take precedence over a style set on its fields.
const WRONG_THEME: &str = r"
const set = (el, name, value) => el.style.setProperty(name, value, 'important');
for (const el of document.querySelectorAll('body, body *')) { set(el, 'background-color', 'transparent'); }
set(document.body, 'background-color', '#ffffff');
for (const el of document.querySelectorAll('h1, p, label, span, a')) { set(el, 'color', '#b0b0b0'); }
const field = document.createElement('input');
field.id = 'wrong-theme-field';
field.setAttribute('aria-label', 'wrong theme');
set(field, 'border', '1px solid #eeeeee');
set(field, 'background-color', '#ffffff');
document.querySelector('main').appendChild(field);
return 'painted';
";

/// Runs one of the colour checks on the page, with the shared arithmetic.
async fn colour(journey: &Journey, check: &str) -> String {
    journey
        .evaluate(&format!("{COLOUR}\n{check}"))
        .await
        .expect("the page runs the colour check")
}

/// Opens `path` and waits for its tab order to hold still.
async fn settle(journey: &Journey, stack: &Stack, path: &str) {
    journey.open(&stack.url(path)).await;
    let mut last = String::new();
    for _ in 0..SETTLE_TRIES {
        let now = journey
            .evaluate(COUNT_FOCUSABLE)
            .await
            .expect("the page counts its controls");
        if now == last {
            return;
        }
        last = now;
        tokio::time::sleep(SETTLE).await;
    }
    panic!("{path} kept drawing new controls, so its tab order never held still");
}

/// Tabs through the open screen and reports every control the keyboard
/// never reached and every stop that drew no ring.
async fn walk(journey: &Journey, path: &str, theme: &str) -> Vec<String> {
    let named = journey
        .evaluate(TAG_FOCUSABLE)
        .await
        .expect("the page tags its controls");
    let controls: Vec<&str> = named.lines().collect();
    assert!(
        !controls.is_empty(),
        "{path} in the {theme} theme offers no control at all, which no screen does"
    );
    let mut reached = vec![false; controls.len()];
    let mut findings = Vec::new();
    journey
        .tab(controls.len() + EXTRA_STOPS)
        .await
        .expect("the browser takes the tab presses");
    let stops = journey
        .evaluate(WALK_STOPS)
        .await
        .expect("the page reads the stops");
    for stop in stops.lines() {
        let mut fields = stop.split('|');
        let (Some(tag), Some(shape), Some(width), Some(named)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if shape == "none" || (shape != "shadow" && width.starts_with('0')) {
            findings.push(format!(
                "{path} ({theme}): focus on {named} draws no ring (outline {shape} {width})"
            ));
        }
        if let Ok(index) = tag.parse::<usize>()
            && let Some(seen) = reached.get_mut(index)
        {
            *seen = true;
        }
    }
    for (index, seen) in reached.iter().enumerate() {
        if !seen {
            let named = controls.get(index).copied().unwrap_or("?");
            findings.push(format!("{path} ({theme}): tabbing never reached {named}"));
        }
    }
    findings
}

/// The whole battery over the open screen in `theme`.
async fn battery(journey: &Journey, stack: &Stack, path: &str, theme: &str) -> Vec<String> {
    settle(journey, stack, path).await;
    let mut findings = Vec::new();
    for (check, script) in [("contrast", CONTRAST), ("control edges", EDGES)] {
        let found = colour(journey, script).await;
        findings.extend(
            found
                .lines()
                .map(|line| format!("{path} ({theme}) {check}: {line}")),
        );
    }
    let semantics = journey
        .evaluate(SEMANTICS)
        .await
        .expect("the page reads its markup");
    findings.extend(semantics.lines().map(|line| format!("{path}: {line}")));
    findings.extend(walk(journey, path, theme).await);
    findings
}

/// Whether `path` scrolls sideways at the reflow width.
async fn reflow(journey: &Journey, stack: &Stack, path: &str) -> Option<String> {
    journey.open(&stack.url(path)).await;
    let over = journey
        .evaluate(SIDEWAYS)
        .await
        .expect("the page reads its width");
    (!over.is_empty()).then(|| format!("{path} at {REFLOW_WIDTH} px scrolls sideways by {over}"))
}

/// Runs the whole battery over every screen `answers` draws, in both themes
/// and at the reflow width, and fails with every finding.
async fn every_screen(answers: Server) {
    let stack = Stack::start_with(answers).await;
    with_journey(async |journey| {
        let mut findings = Vec::new();
        for path in SIGN_IN_SCREENS {
            findings.extend(battery(journey, &stack, path, "light").await);
        }
        sign_in(journey, &stack).await;
        for theme in THEMES {
            choose(journey, &stack, "theme", theme, "data-theme").await;
            for path in SCREENS {
                findings.extend(battery(journey, &stack, path, theme).await);
            }
        }
        journey.resize(REFLOW_WIDTH, WINDOW.1).await;
        for path in SCREENS {
            findings.extend(reflow(journey, &stack, path).await);
        }
        journey.resize(WINDOW.0, WINDOW.1).await;
        assert!(
            findings.is_empty(),
            "the AA battery over the {answers:?} screens found {} failures:\n{}",
            findings.len(),
            findings.join("\n")
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_screens_refused_state_passes_the_aa_battery_in_both_themes() {
    if !journeys_enabled() {
        return;
    }
    every_screen(Server::Refusing).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_screens_filled_state_passes_the_aa_battery_in_both_themes() {
    if !journeys_enabled() {
        return;
    }
    every_screen(Server::Filled).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_colour_checks_refuse_a_deliberately_wrong_theme() {
    if !journeys_enabled() {
        return;
    }
    let stack = Stack::start().await;
    with_journey(async |journey| {
        sign_in(journey, &stack).await;
        settle(journey, &stack, "/search").await;
        assert_eq!(
            "",
            colour(journey, CONTRAST).await,
            "the shipped theme passes before it is broken"
        );
        journey
            .evaluate(WRONG_THEME)
            .await
            .expect("the page takes the wrong theme");
        let contrast = colour(journey, CONTRAST).await;
        assert!(
            contrast.contains("needs 4.5:1") || contrast.contains("needs 3:1"),
            "grey words on white pass the contrast check:\n{contrast}"
        );
        let edges = colour(journey, EDGES).await;
        assert!(
            edges.contains("wrong-theme-field") && edges.contains("need 3:1"),
            "a pale field edge on white passes the control edge check:\n{edges}"
        );
    })
    .await;
}
