// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The pages the console draws around `ferrohealth-session`'s sign-in.
//!
//! A stopped sign-in is answered in the request's locale, and the sign-in
//! and the sign-out land on the console's own pages. No specification
//! governs the console: our own design.

use axum::response::{IntoResponse as _, Response};
use ferrohealth_session::routes::{Pages, Stopped};
use ferrohealth_smart::oidc::OidcError;
use ferrohealth_ui::i18n::Locale;
use http::header::CACHE_CONTROL;
use http::request::Parts;

use crate::app::SIGN_IN;
use crate::locale::messages::Msg;
use crate::locale::{catalog, negotiated, tr_in};

/// Where a sign-out lands when the provider has no end-session endpoint.
pub const SIGNED_OUT: &str = "/sign-in/signed-out";

/// The console's [`Pages`]: a stopped sign-in answered in plain text from
/// the catalog, in the locale the request negotiates.
#[derive(Debug, Clone)]
pub struct SignInPages {
    /// The locale a request falls back to.
    locale: Locale,
}

impl SignInPages {
    /// The pages of a console whose fallback locale is `locale`.
    #[must_use]
    pub const fn new(locale: Locale) -> Self {
        Self { locale }
    }
}

impl Pages for SignInPages {
    fn stopped(&self, parts: Parts, why: Stopped) -> impl Future<Output = Response> + Send {
        let locale = negotiated(&parts.headers, self.locale.clone());
        let msg = match &why {
            Stopped::NotConfigured => Msg::SignInNotConfigured,
            Stopped::NoSignInPending => Msg::NoSignInPending,
            Stopped::StateMismatch => Msg::SignInStateMismatch,
            Stopped::NoCode => Msg::RedirectCarriesNoCode,
            Stopped::Rejected(OidcError::Authorization { .. }) => Msg::ProviderDidNotSignIn,
            Stopped::Rejected(_) => Msg::IdTokenDidNotVerify,
            Stopped::NotCompleted(_) => Msg::SignInNotCompleted,
            Stopped::Full => Msg::SessionsFull,
            Stopped::Store(_) if parts.uri.path() == ferrohealth_session::paths::LOGOUT => {
                Msg::SessionNotEnded
            }
            Stopped::Store(_) if ferrohealth_session::paths::is_own(parts.uri.path()) => {
                Msg::SignInSessionNotBegun
            }
            _ => Msg::SessionUnreadable,
        };
        let text = tr_in(catalog(), &locale, msg);
        std::future::ready((why.status(), [(CACHE_CONTROL, "no-store")], text).into_response())
    }

    fn session_gone(&self, parts: &Parts) -> String {
        let locale = negotiated(&parts.headers, self.locale.clone());
        tr_in(catalog(), &locale, Msg::SessionGone)
    }

    fn sign_in_page(&self) -> &'static str {
        SIGN_IN
    }

    fn signed_out(&self) -> &'static str {
        SIGNED_OUT
    }

    fn home(&self) -> &'static str {
        "/"
    }
}
