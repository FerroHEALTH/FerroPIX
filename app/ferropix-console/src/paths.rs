// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The addresses the console serves beside its pages.
//!
//! They are the marks, and which pages draw for a reader with no session.
//! The sign-in, callback, sign-out and session routes are
//! `ferrohealth_session::paths`. No specification governs the console: our
//! own design.

use crate::app::SIGN_IN;

/// The path of the favicon.
pub const FAVICON: &str = "/favicon.svg";

/// The path of the brand mark in the light theme.
pub const MARK_LIGHT: &str = "/ferropix-icon.svg";

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
