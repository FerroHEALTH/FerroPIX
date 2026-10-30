// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The addresses the console serves beside its pages: the marks, the
//! sign-in and session routes, and which pages draw for a reader with no
//! session. No specification governs the console: our own design.

use crate::app::SIGN_IN;

/// The path of the favicon.
pub const FAVICON: &str = "/favicon.svg";

/// The path of the brand mark in the light theme.
pub const MARK_LIGHT: &str = "/ferropix-icon.svg";

/// The path of the sign-in route, which sends the steward to the OpenID
/// Provider.
pub const LOGIN: &str = "/login";

/// The session route the page's session watch asks: `GET` reads what the
/// session has left, `POST` extends it (FerroUI `docs/consoles.md`, "The
/// session's end").
pub const SESSION: &str = "/session";

/// The path of the brand mark in the dark theme.
pub const MARK_DARK: &str = "/ferropix-icon-dark.svg";

/// Whether `path` is a sign-in page, drawn for a reader with no session.
#[must_use]
pub fn is_sign_in(path: &str) -> bool {
    path == SIGN_IN
        || path
            .strip_prefix(SIGN_IN)
            .is_some_and(|rest| rest.starts_with('/'))
}
