// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The console's own single-key shortcuts, bound once the page has
//! hydrated (FerroUI `view::dock`: "the console binds the key").
//!
//! On the review queue `o` and `d` open the focused row's side panel. On
//! any page a key in [`KEYS`] presses the control whose
//! `aria-keyshortcuts` names it, or puts the focus in the field that does,
//! such as a decision of the docked bar or its reason. A key pressed while
//! typing, with a modifier held, or in the phone layout does nothing, as
//! FerroUI's tests say (family rule 6). No specification governs the
//! console's keys: our own design.

/// The keys this listener takes; every other key is FerroUI's or the
/// browser's.
pub const KEYS: [&str; 7] = ["o", "d", "l", "n", "r", "[", "]"];

/// Listens for the console's keys; draw it once, inside the router.
pub(super) fn listen_keys() {
    #[cfg(target_arch = "wasm32")]
    {
        use leptos::prelude::*;
        Effect::new(move |_| {
            let handle = window_event_listener(leptos::ev::keydown, |event| pressed(&event));
            on_cleanup(move || handle.remove());
        });
    }
}

/// The selector of the element `key` reaches: the focused queue row's
/// side panel opener for `o` and `d`, else the enabled control whose
/// `aria-keyshortcuts` names the key; `None` for a key not in [`KEYS`].
#[must_use]
pub fn selectors(key: &str) -> Option<Vec<String>> {
    if !KEYS.contains(&key) {
        return None;
    }
    let mut found = Vec::new();
    if matches!(key, "o" | "d") {
        found.push(String::from(
            ".fui-worklist-grid tr[aria-current='true'] button[command='show-modal']:not([hidden])",
        ));
    }
    // NOTE: WAI-ARIA 1.2, `aria-keyshortcuts`: a letter key is named by its upper-case
    // character, as FerroUI's `view::kbd::aria_keys` writes it.
    let named = ferrohealth_ui::view::kbd::aria_keys(key)?;
    found.push(format!("[aria-keyshortcuts~=\"{named}\"]:not([disabled])"));
    Some(found)
}

/// Presses or focuses what the key of `event` reaches.
#[cfg(target_arch = "wasm32")]
fn pressed(event: &leptos::ev::KeyboardEvent) {
    use ferrohealth_ui::widget::shortcuts::{chorded, phone_layout, typing};
    use leptos::wasm_bindgen::JsCast as _;
    if event.default_prevented() || typing(event) || chorded(event) || phone_layout() {
        return;
    }
    let Some(selectors) = selectors(&event.key()) else {
        return;
    };
    let document = leptos::prelude::document();
    let Some(target) = selectors.iter().find_map(|selector| {
        document
            .query_selector(selector)
            .ok()
            .flatten()
            .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
    }) else {
        return;
    };
    event.prevent_default();
    let field = matches!(
        target.tag_name().to_ascii_lowercase().as_str(),
        "input" | "select" | "textarea"
    );
    if field {
        if target.focus().is_err() {
            leptos::logging::warn!("the browser refused the focus");
        }
    } else {
        // NOTE: HTML Living Standard, the `dialog` element: a modal returns the focus to the
        // element that held it when it opened, so the control the key pressed takes it first.
        if target.focus().is_err() {
            leptos::logging::warn!("the browser refused the focus");
        }
        target.click();
    }
}

#[cfg(test)]
mod tests {
    use super::selectors;

    #[test]
    fn a_key_reaches_the_control_that_names_it_and_o_and_d_the_focused_row_first() {
        let open = selectors("o").expect("o is the console's");
        assert_eq!(2, open.len());
        assert!(open[0].contains("aria-current"));
        assert_eq!(
            vec![String::from("[aria-keyshortcuts~=\"L\"]:not([disabled])")],
            selectors("l").expect("l is the console's")
        );
        assert_eq!(None, selectors("?"), "the legend's key is FerroUI's");
        assert_eq!(None, selectors("/"), "the search's key is FerroUI's");
    }
}
