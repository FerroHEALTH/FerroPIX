// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The page for an address the console does not serve (design 30): the
//! status in words before its number, what the address may mean, and the
//! ways on, FerroUI's error page in the shell.

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::view::error_page::{ErrorPage, Status, Way, error_page};
use leptos::prelude::*;

use crate::app::{DASHBOARD, SEARCH};
use crate::locale::{Msg, tr};
use crate::pages::title;

/// The not-found page, answered `404`.
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn NotFound() -> impl IntoView {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(response) = use_context::<leptos_axum::ResponseOptions>() {
        response.set_status(http::StatusCode::NOT_FOUND);
    }
    let page = error_page(ErrorPage {
        status: Status::NotFound,
        title: tr(Msg::NotFoundTitle),
        body: tr(Msg::NotFoundBody).into_any(),
        hint: None,
        ways: vec![
            Way {
                label: tr(Msg::NotFoundBack),
                href: String::from(DASHBOARD),
                glyph: Some(Glyph::ARROW_LEFT),
            },
            Way {
                label: tr(Msg::NotFoundSearch),
                href: String::from(SEARCH),
                glyph: Some(Glyph::SEARCH),
            },
        ],
        detail: None,
    });
    view! {
        {title(tr(Msg::NotFoundTitle))}
        {page}
    }
    .into_any()
}
