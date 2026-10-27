// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The forms the browser posts in place once the page has hydrated.
//!
//! A search, a trail filter, a reveal, an identifier domain's form, the
//! pseudonymisation domains' forms and the queue's filters, paging and
//! deferral are each drawn from their answer with no new document. No
//! specification governs the console's pages: our own design.

use leptos::prelude::*;

use crate::model::PageContext;

/// The page context a post drawn in the page brought, which the page shows
/// in place of a read until the next navigation.
#[derive(Debug, Clone, Copy)]
pub struct Posted(pub RwSignal<Option<PageContext>>);

/// The paths whose plain post draws a page the browser can draw in place:
/// a search, a trail filter, a reveal, an identifier domain's form, the
/// pseudonymisation domains' forms and the queue's forms.
#[must_use]
pub fn posts_in_page(path: &str) -> bool {
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    matches!(
        parts.as_slice(),
        ["search" | "audit" | "domains" | "pseudonym-domains" | "queue"]
            | ["search", "demographics"]
            | ["domains", _]
    ) || matches!(parts.as_slice(), ["patients", _, last] if *last == crate::pages::patient::REVEAL)
}

/// Takes the submit of every form [`posts_in_page`] names through
/// [`post_now`](super::post_now), drawing its answer in place of the plain post's new
/// document; draw it once, inside the router.
pub(super) fn listen_posts() {
    #[cfg(target_arch = "wasm32")]
    {
        let posted = use_context::<Posted>();
        let navigate = leptos_router::hooks::use_navigate();
        Effect::new(move |_| {
            let navigate = navigate.clone();
            let handle = window_event_listener(leptos::ev::submit, move |event| {
                post_in_page(&event, posted, navigate.clone());
            });
            on_cleanup(move || handle.remove());
        });
    }
}

/// Posts the form under `event` through [`post_now`](super::post_now) and draws its answer
/// in the page, when its path is one [`posts_in_page`] names.
#[cfg(target_arch = "wasm32")]
fn post_in_page(
    event: &leptos::ev::SubmitEvent,
    posted: Option<Posted>,
    navigate: impl Fn(&str, leptos_router::NavigateOptions) + 'static,
) {
    use leptos::wasm_bindgen::{JsCast as _, JsValue};
    if event.default_prevented() {
        return;
    }
    let (Some(Posted(posted)), Some(form)) = (
        posted,
        event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::HtmlFormElement>().ok()),
    ) else {
        return;
    };
    let action = form.action();
    let path = web_sys::Url::new(&action).map_or(action, |url| url.pathname());
    if !form.method().eq_ignore_ascii_case("post")
        || form.has_attribute(ferrohealth_ui::widget::submit::RELOADS)
        || !posts_in_page(&path)
    {
        return;
    }
    let Ok(data) = web_sys::FormData::new_with_form(&form) else {
        return;
    };
    event.prevent_default();
    if let Some(by) = event
        .submitter()
        .and_then(|by| by.dyn_into::<web_sys::HtmlElement>().ok())
        && let (Some(name), Some(value)) = (by.get_attribute("name"), by.get_attribute("value"))
        && data.append_with_str(&name, &value).is_err()
    {
        leptos::logging::warn!("the browser refused the submit's value");
    }
    let mut fields = Vec::new();
    if let Ok(entries) = JsValue::from(data.entries()).dyn_into::<web_sys::js_sys::Iterator>() {
        while let Ok(next) = entries.next() {
            if next.done() {
                break;
            }
            let pair = web_sys::js_sys::Array::from(&next.value());
            if let (Some(name), Some(value)) = (pair.get(0).as_string(), pair.get(1).as_string()) {
                fields.push((name, value));
            }
        }
    }
    leptos::task::spawn_local(async move {
        match super::post_now(path, fields).await {
            Ok(page) => {
                if let Some(to) = &page.redirect {
                    navigate(to, leptos_router::NavigateOptions::default());
                    return;
                }
                let shown = page.path.clone();
                drop(posted.try_set(Some(page)));
                // NOTE: no specification governs this: our own design; a form posted from a
                // dialog closes it, as the new document a plain post loads would.
                if let Some(dialog) = form
                    .closest("dialog")
                    .ok()
                    .flatten()
                    .and_then(|dialog| dialog.dyn_into::<web_sys::HtmlDialogElement>().ok())
                {
                    dialog.close();
                }
                if window().location().pathname().ok().as_deref() != Some(shown.as_str()) {
                    navigate(&shown, leptos_router::NavigateOptions::default());
                }
                ferrohealth_ui::widget::submit::drawn();
            }
            Err(error) => {
                leptos::logging::warn!("the post did not come back: {error}");
                if form.submit().is_err() {
                    leptos::logging::warn!("the browser refused the plain post");
                }
            }
        }
    });
}
