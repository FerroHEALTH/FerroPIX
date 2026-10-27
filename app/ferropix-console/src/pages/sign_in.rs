// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The sign-in page at `/sign-in` (design 14, 14.4, 14.5).
//!
//! FerroUI's sign-in card in the centred layout, with the provider's
//! action that starts the OpenID Connect sign-in at `GET /login`. No
//! patient data is drawn before a session exists.
//!
//! `/sign-in/signed-out` is where a sign-out leads, and
//! `/sign-in/expired` where a request with an ended session is sent. A
//! console with no identity provider configured draws the card with its
//! action disabled and says so.

use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::view::shell::{brand_mark, centred, lockup_plain};
use ferrohealth_ui::view::sign_in::{Notice, Provider, SignIn, State, sign_in};
use leptos::prelude::*;

use crate::app::shell::footer;
use crate::locale::{Msg, tr};
use crate::pages::title;
use crate::paths::{MARK_DARK, MARK_LIGHT};

/// The sign-in page, in the state its route names.
#[component]
#[expect(
    clippy::must_use_candidate,
    reason = "the #[component] macro drops a #[must_use] from the function it writes"
)]
pub fn SignInPage() -> impl IntoView {
    let configured = crate::app::page_context().sign_in;
    let named = crate::app::param("state");
    let state = if configured {
        match named.as_deref() {
            Some("signed-out") => State::SignedOut(Notice::new(tr(Msg::SignedOut))),
            Some("expired") => State::Expired(Notice::new(tr(Msg::SessionExpired))),
            _ => State::Ready,
        }
    } else {
        State::Unavailable(Notice::new(tr(Msg::SignInUnavailable)))
    };
    let card = sign_in(SignIn {
        lockup: lockup_plain(
            brand_mark(MARK_LIGHT, MARK_DARK),
            tr(Msg::Product),
            tr(Msg::ConsoleWord),
        )
        .into_any(),
        title: Some(tr(Msg::SignInTitle)),
        state,
        provider: Some(Provider {
            href: String::from(crate::paths::LOGIN),
            label: tr(Msg::SignInWith),
            busy: tr(Msg::SignInTaking),
            glyph: Glyph::LOG_IN,
        }),
        basic: None,
        line: None,
        note: None,
        links: vec![(
            tr(Msg::Documentation),
            String::from("https://ferropix.eu/docs/"),
        )],
        caption: Some(tr(Msg::SignInCaption)),
    })
    .into_any();
    view! {
        {title(tr(Msg::SignIn))}
        {centred(card, footer())}
    }
}
