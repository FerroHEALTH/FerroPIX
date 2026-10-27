// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The page the browser draws in place, followed on the root element.
//!
//! That is its sign-in redirect, its language, and the steward's
//! preferences. No specification governs the console's pages: our own
//! design.

#[cfg(target_arch = "wasm32")]
use ferrohealth_ui::theme::Preferences;
#[cfg(target_arch = "wasm32")]
use leptos::prelude::*;

#[cfg(target_arch = "wasm32")]
use super::{DRAWS, locale_of};
use crate::model::PageContext;

/// Follows the page `page` the browser now draws: sends the browser to the
/// sign-in it names as its redirect, and sets the root element's theme,
/// density, motion and hints from its preferences, as the server writes
/// them on a document it draws; a new language loads the page as a
/// document, since its words are drawn in its locale.
pub(super) fn follow(page: &PageContext) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(to) = &page.redirect {
            // NOTE: FerroUI `docs/consoles.md`, "The session's end": a session the console's
            // own read finds gone is said to every tab, so each leaves for sign-in.
            ferrohealth_ui::widget::session::gone();
            if window().location().assign(to).is_err() {
                leptos::logging::warn!("the browser refused the sign-in");
            }
            return;
        }
        let Some(root) = document().document_element() else {
            return;
        };
        let language = locale_of(page).tag();
        if root
            .get_attribute("lang")
            .is_some_and(|lang| lang != language)
        {
            if window().location().reload().is_err() {
                leptos::logging::warn!("the browser refused to load the page in its language");
            }
            return;
        }
        let draws = root
            .get_attribute(DRAWS)
            .and_then(|draws| draws.parse::<u32>().ok())
            .unwrap_or_default()
            .wrapping_add(1);
        if root.set_attribute(DRAWS, &draws.to_string()).is_err() {
            leptos::logging::warn!("the browser refused the root's {DRAWS}");
        }
        let chosen = Preferences::read(&page.preferences, &page.prefix);
        for (name, value) in [
            ("data-theme", chosen.theme.key()),
            ("data-density", chosen.density.key()),
            ("data-motion", chosen.motion.key()),
            ("data-hints", chosen.hints.key()),
        ] {
            if root.get_attribute(name).as_deref() != Some(value)
                && root.set_attribute(name, value).is_err()
            {
                leptos::logging::warn!("the browser refused the root's {name}");
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _: &PageContext = page;
}
