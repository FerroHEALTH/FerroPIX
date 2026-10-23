// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pages as a browser receives them, written out for a capture.
//!
//! Behind `FERROPIX_CAPTURES`, a directory, every page is drawn signed in
//! as the synthetic steward against a FerroPIX server that does not answer,
//! in the light and the dark theme, and written there with the stylesheet,
//! the theme and the marks beside it, so a headless browser draws each one
//! at 1440 and 390 px to set it beside its mockup in FerroUI's
//! `design/consoles/ferropix/`. Without the variable the test writes
//! nothing. No specification governs the captures: our own design.

use std::error::Error;
use std::path::PathBuf;

use crate::support::{console, get, get_with, offline, send, steward_session};

/// The pages captured, each with the name its files take.
const PAGES: [(&str, &str); 16] = [
    ("dashboard", "/"),
    ("queue", "/queue"),
    ("pair", "/queue/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"),
    ("search", "/search"),
    ("search-demographics", "/search/demographics?searched"),
    ("patient", "/patients/0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"),
    ("sources", "/sources"),
    ("domains", "/domains"),
    ("pseudonym-domains", "/pseudonym-domains"),
    ("configuration", "/configuration"),
    ("audit", "/audit"),
    ("about", "/about"),
    ("preferences", "/preferences"),
    ("preferences-language", "/preferences/language"),
    ("not-found", "/patients/123456789"),
    ("sign-in", "/sign-in"),
];

/// The files a captured page links, each written beside it.
const FILES: [&str; 5] = [
    "/ferrohealth-ui.css",
    "/theme.css",
    "/favicon.svg",
    "/ferropix-icon.svg",
    "/ferropix-icon-dark.svg",
];

#[tokio::test]
async fn every_page_is_written_out_for_a_capture_when_asked() -> Result<(), Box<dyn Error>> {
    let Some(folder) = std::env::var_os("FERROPIX_CAPTURES").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&folder)?;
    let (state, service) = console(&offline("http://127.0.0.1:9/"))?;
    let session = steward_session(&state);
    for file in FILES {
        let (_, body) = send(&service, get(file)?).await?;
        std::fs::write(folder.join(file.trim_start_matches('/')), body)?;
    }
    for theme in ["light", "dark"] {
        let cookie = format!("{session}; ferropix_console_theme={theme}");
        for (name, route) in PAGES {
            let (_, page) = send(&service, get_with(route, &cookie)?).await?;
            let page = FILES.iter().fold(page, |page, file| {
                page.replace(
                    &format!("\"{file}\""),
                    &format!("\"{}\"", file.trim_start_matches('/')),
                )
            });
            std::fs::write(folder.join(format!("{name}-{theme}.html")), page)?;
        }
    }
    Ok(())
}
