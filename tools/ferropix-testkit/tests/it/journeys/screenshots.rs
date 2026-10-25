// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The book's screenshots of the operator console, taken behind the
//! `FERROPIX_SCREENSHOTS` gate, and the guard that every console route has
//! them.
//!
//! With `FERROPIX_SCREENSHOTS=1` the capture signs in to the filled stack
//! and takes every shot of [`SHOTS`] in the light and the dark theme at 1440
//! CSS pixels wide and the page's full height, as `<image>.png` and
//! `<image>-dark.png` under [`IMAGES`], and again at 390 CSS pixels, a
//! phone, under the ignored [`NARROW_IMAGES`] for the visual review. It then
//! writes the release it ran at to [`CAPTURED`]. Without the gate it returns
//! at once.
//!
//! The guard runs in the ordinary suite: every route of the console's router
//! is shown by a shot, every shot has both images, every image is a shot's,
//! the book shows every light image and links every dark one, and
//! [`CAPTURED`] names the workspace version, so a release fails until the
//! images are taken again. No specification governs the screenshots: our
//! own design, after FerroBRIDGE's.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::json;
use thirtyfour::By;

use crate::journeys::browser::{Journey, with_journey, workspace};
use crate::journeys::preferences::choose;
use crate::journeys::sign_in::sign_in;
use crate::journeys::stack::{Server, Stack};
use crate::journeys::{NOT_FOUND, routes, serves};

/// The environment variable that opts in to the capture.
const SCREENSHOTS_GATE: &str = "FERROPIX_SCREENSHOTS";

/// The book's folder of console screenshots, from the workspace root.
const IMAGES: &str = "website/book/src/operate/img/console";

/// The file beside the images that names the release they show.
const CAPTURED: &str = "captured-version.txt";

/// The ignored folder of the phone-width screenshots, from the workspace
/// root.
const NARROW_IMAGES: &str = "target/console-screenshots/narrow";

/// The height of the window a page is laid out in before its full height is
/// measured.
const HEIGHT: u32 = 900;

/// The tallest screenshot, so a page that grows without bound stays a file
/// the book can hold.
const TALLEST: u32 = 8000;

/// Every shot: the image's name, without the theme suffix and the
/// extension, and the address the browser loads. The three sign-in shots
/// are taken signed out.
const SHOTS: [(&str, &str); 23] = [
    ("sign-in", "/sign-in"),
    ("signed-out", "/sign-in/signed-out"),
    ("expired", "/sign-in/expired"),
    ("dashboard", "/"),
    ("queue", "/queue"),
    ("queue-pair", "/queue/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"),
    ("search", "/search"),
    ("search-demographics", "/search/demographics"),
    ("patient", "/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"),
    (
        "patient-merge",
        "/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b/merge",
    ),
    (
        "patient-unmerge",
        "/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b/unmerge",
    ),
    ("sources", "/sources"),
    ("source", "/sources/EAST.ADT"),
    ("domains", "/domains"),
    ("domain", "/domains/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5c"),
    ("pseudonym-domains", "/pseudonym-domains"),
    ("configuration", "/configuration"),
    ("audit", "/audit"),
    ("about", "/about"),
    ("preferences", "/preferences"),
    ("preferences-language", "/preferences/language"),
    ("preferences-keyboard", "/preferences/keyboard"),
    ("not-found", NOT_FOUND),
];

/// How many shots are taken signed out, from the start of [`SHOTS`].
const SIGNED_OUT_SHOTS: usize = 3;

/// The two themes, as the theme cookie holds them, with what each image's
/// name carries.
const THEMES: [(&str, &str); 2] = [("light", ""), ("dark", "-dark")];

/// The two widths in CSS pixels: the book's and a phone's.
const WIDTHS: [u32; 2] = [1440, 390];

/// The part of the Chrome `DevTools` Protocol's `Page.getLayoutMetrics`
/// answer the capture reads.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutMetrics {
    /// The size of the page's content, in CSS pixels.
    css_content_size: ContentSize,
}

/// A size in CSS pixels.
#[derive(Debug, Deserialize)]
struct ContentSize {
    /// The height.
    height: f64,
}

/// The file of `image` with the theme suffix `suffix` at `width`: the
/// book's folder for the wide one, the ignored folder for the narrow one.
fn output(image: &str, suffix: &str, width: u32) -> PathBuf {
    let folder = if width == WIDTHS[0] {
        IMAGES
    } else {
        NARROW_IMAGES
    };
    workspace()
        .join(folder)
        .join(format!("{image}{suffix}.png"))
}

/// Lays the page out in a window `height` CSS pixels tall at `width`.
async fn window(journey: &Journey, width: u32, height: u32) {
    journey
        .driver()
        .cdp()
        .send_raw(
            "Emulation.setDeviceMetricsOverride",
            json!({ "width": width, "height": height, "deviceScaleFactor": 1, "mobile": width < 500 }),
        )
        .await
        .unwrap_or_else(|error| panic!("laying the page out at {width}x{height}: {error}"));
}

/// Has the browser report `theme` as the system's colour scheme, so the
/// controls the browser draws match the page.
async fn scheme(journey: &Journey, theme: &str) {
    journey
        .driver()
        .cdp()
        .send_raw(
            "Emulation.setEmulatedMedia",
            json!({ "features": [{ "name": "prefers-color-scheme", "value": theme }] }),
        )
        .await
        .unwrap_or_else(|error| panic!("emulating the {theme} colour scheme: {error}"));
}

/// The height of the page's content, in whole CSS pixels from [`HEIGHT`] to
/// [`TALLEST`].
async fn content_height(journey: &Journey) -> u32 {
    let metrics = journey
        .driver()
        .cdp()
        .send_raw("Page.getLayoutMetrics", json!({}))
        .await
        .unwrap_or_else(|error| panic!("reading the page's layout: {error}"));
    let metrics: LayoutMetrics = serde_json::from_value(metrics)
        .unwrap_or_else(|error| panic!("the layout answer reads: {error}"));
    let content = metrics.css_content_size.height;
    (HEIGHT..=TALLEST)
        .find(|tall| f64::from(*tall) >= content)
        .unwrap_or(TALLEST)
}

/// Opens `address`, waits for its heading, and writes it at its full height
/// as `image` with `suffix` at `width`.
async fn capture(
    journey: &Journey,
    stack: &Stack,
    (image, address): (&str, &str),
    (suffix, width): (&str, u32),
) {
    journey.open(&stack.url(address)).await;
    journey.element(By::Css("h1"), "the page's heading").await;
    let height = content_height(journey).await;
    window(journey, width, height).await;
    journey
        .evaluate("window.scrollTo(0, 0); return 'top';")
        .await
        .unwrap_or_else(|error| panic!("scrolling {address} to its top: {error}"));
    let png = journey
        .driver()
        .screenshot_as_png()
        .await
        .unwrap_or_else(|error| panic!("taking {image}: {error}"));
    window(journey, width, HEIGHT).await;
    let file = output(image, suffix, width);
    std::fs::write(&file, png)
        .unwrap_or_else(|error| panic!("writing {}: {error}", file.display()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_book_shows_every_console_page_in_light_and_dark() {
    if !std::env::var(SCREENSHOTS_GATE).is_ok_and(|value| value == "1") {
        return;
    }
    for folder in [IMAGES, NARROW_IMAGES] {
        let folder = workspace().join(folder);
        std::fs::create_dir_all(&folder)
            .unwrap_or_else(|error| panic!("creating {}: {error}", folder.display()));
    }
    let (signed_out, signed_in) = SHOTS.split_at(SIGNED_OUT_SHOTS);
    for width in WIDTHS {
        for (theme, suffix) in THEMES {
            let stack = Stack::start_with(Server::Filled).await;
            with_journey(async |journey| {
                window(journey, width, HEIGHT).await;
                scheme(journey, theme).await;
                journey.open(&stack.url("/sign-in")).await;
                journey
                    .set_cookie(
                        &format!("{}theme", crate::journeys::stack::PREFERENCE_PREFIX),
                        theme,
                    )
                    .await
                    .unwrap_or_else(|error| panic!("setting the theme cookie: {error}"));
                for shot in signed_out {
                    capture(journey, &stack, *shot, (suffix, width)).await;
                }
                sign_in(journey, &stack).await;
                choose(journey, &stack, "theme", theme, "data-theme").await;
                for shot in signed_in {
                    capture(journey, &stack, *shot, (suffix, width)).await;
                }
            })
            .await;
        }
    }
    let captured = workspace().join(IMAGES).join(CAPTURED);
    std::fs::write(&captured, format!("{}\n", env!("CARGO_PKG_VERSION")))
        .unwrap_or_else(|error| panic!("writing {}: {error}", captured.display()));
}

/// The text of every page of the book.
fn book() -> String {
    let mut text = String::new();
    let mut folders = vec![workspace().join("website/book/src")];
    while let Some(folder) = folders.pop() {
        let entries = std::fs::read_dir(&folder)
            .unwrap_or_else(|error| panic!("reading {}: {error}", folder.display()));
        for entry in entries {
            let entry = entry.expect("a directory entry").path();
            if entry.is_dir() {
                folders.push(entry);
            } else if entry.extension().is_some_and(|extension| extension == "md") {
                text.push_str(&std::fs::read_to_string(&entry).expect("a page of the book"));
            }
        }
    }
    text
}

#[test]
fn every_console_route_has_a_screenshot_in_light_and_dark() {
    for route in routes() {
        assert!(
            SHOTS.iter().any(|(_, address)| serves(&route, address)),
            "the console route {route} has no screenshot: add a shot for it to journeys::screenshots and run the capture with {SCREENSHOTS_GATE}=1"
        );
    }
    let folder = workspace().join(IMAGES);
    let mut images = BTreeSet::new();
    for (image, _) in SHOTS {
        for (_, suffix) in THEMES {
            let file = output(image, suffix, WIDTHS[0]);
            assert!(
                file.is_file(),
                "{} is missing: run the capture with {SCREENSHOTS_GATE}=1",
                file.display()
            );
            images.insert(file);
        }
    }
    let captured = folder.join(CAPTURED);
    images.insert(captured.clone());
    let version = std::fs::read_to_string(&captured).unwrap_or_else(|error| {
        panic!(
            "{} cannot be read ({error}): run the capture with {SCREENSHOTS_GATE}=1",
            captured.display()
        )
    });
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        version.trim_end(),
        "the console screenshots were taken at another release: run the capture with {SCREENSHOTS_GATE}=1"
    );
    let entries = std::fs::read_dir(&folder)
        .unwrap_or_else(|error| panic!("reading {}: {error}", folder.display()));
    for entry in entries {
        let entry = folder.join(entry.expect("a directory entry").file_name());
        assert!(
            images.contains(&entry),
            "{} is no shot's image: remove it",
            entry.display()
        );
    }
    let book = book();
    for (image, _) in SHOTS {
        let light = format!("img/console/{image}.png");
        let dark = format!("img/console/{image}-dark.png");
        assert!(
            book.contains(&format!("]({light})")),
            "no page of the book shows {light}"
        );
        assert!(
            book.contains(&format!("]({dark})")),
            "no page of the book links {dark}"
        );
    }
}
