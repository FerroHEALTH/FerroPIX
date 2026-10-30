// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The sign-in page at `/sign-in` (design 14, 14.4, 14.5).
//!
//! FerroUI's sign-in card in the centred layout, with the provider's
//! action that starts the OpenID Connect sign-in at `GET /login`. No
//! patient data is drawn before a session exists.
//!
//! `/sign-in/signed-out` is where a sign-out leads, and
//! `/sign-in/expired` where a request with an ended session is sent; both
//! tell every other tab of the console that the session has ended. A
//! `return` in the query, one of the console's own pages, rides the
//! provider's action, so the sign-in leads back to it. A console with no
//! identity provider configured draws the card with its action disabled and
//! says so.

use ferrohealth_ui::html::Html;
use ferrohealth_ui::icon::Glyph;
use ferrohealth_ui::view::session::{session_ended, sign_in_address};
use ferrohealth_ui::view::shell::{brand_mark, lockup_plain};
use ferrohealth_ui::view::sign_in::{Notice, Provider, SignIn, State, sign_in};

use crate::locale::messages::Msg;
use crate::pages::Drawn;
use crate::paths::{MARK_DARK, MARK_LIGHT};
use crate::visit::Visit;

/// The sign-in page, in the state its route names.
#[must_use]
pub fn sign_in_page(visit: &Visit) -> Drawn {
    let page = &visit.page;
    let named = visit.param("state");
    // NOTE: FerroUI `docs/consoles.md`, "The session's end": the signed-out page tells every
    // other tab the session has ended, as the page a gone session is sent to does here.
    let ended = if matches!(named.as_deref(), Some("signed-out" | "expired")) {
        session_ended()
    } else {
        Html::empty()
    };
    let login = page.back.as_deref().map_or_else(
        || String::from(crate::paths::LOGIN),
        |back| sign_in_address(crate::paths::LOGIN, back),
    );
    let state = if page.sign_in {
        match named.as_deref() {
            Some("signed-out") => State::SignedOut(Notice::new(visit.tr(Msg::SignedOut))),
            Some("expired") => State::Expired(Notice::new(visit.tr(Msg::SessionExpired))),
            _ => State::Ready,
        }
    } else {
        State::Unavailable(Notice::new(visit.tr(Msg::SignInUnavailable)))
    };
    let card = sign_in(
        &visit.locale,
        SignIn {
            lockup: lockup_plain(
                brand_mark(MARK_LIGHT, MARK_DARK),
                visit.tr(Msg::Product),
                visit.tr(Msg::ConsoleWord),
            ),
            title: Some(visit.tr(Msg::SignInTitle)),
            state,
            provider: Some(Provider {
                href: login,
                label: visit.tr(Msg::SignInWith),
                busy: visit.tr(Msg::SignInTaking),
                glyph: Glyph::LOG_IN,
            }),
            basic: None,
            line: None,
            note: None,
            links: vec![(
                visit.tr(Msg::Documentation),
                String::from("https://ferropix.eu/docs/"),
            )],
            caption: Some(visit.tr(Msg::SignInCaption)),
        },
    );
    Drawn::new(visit.tr(Msg::SignIn), card.then(ended))
}
