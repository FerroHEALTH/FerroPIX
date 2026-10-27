// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FerroPIX Console, the data steward's operator console: a Leptos app
//! the server draws and the browser hydrates whole, with a backend-for-frontend
//! in front of the FerroPIX server.
//!
//! The console is one more client of the FerroPIX server: its server half
//! reaches the server through [`pix`], server to server, with the signed-in
//! steward's own access token, and keeps no patient data of its own. It
//! holds the steward's sign-in session on the server ([`session`],
//! [`oidc`]), so the browser carries an opaque cookie and never a token.
//! [`app`] is the page tree, drawn in the FerroUI shell with the pages of
//! [`pages`] in the steward's preferences ([`preferences`]) and the
//! request's [`locale`]; [`server`] serves it with the health, sign-in and
//! preference routes under the browser security headers, [`config`] reads
//! the TOML configuration, and [`cli`] and [`command`] run the binary over
//! the subcommands and the probe of `ferrohealth-server`. No
//! specification governs the console: our own design (`docs/architecture.md`
//! §12).
//!
//! The server half compiles for the host. The browser half, compiled for
//! `wasm32-unknown-unknown`, hydrates the page the server drew and draws
//! every page after it ([`app`], [`pages`], [`model`], [`locale`],
//! [`preferences`]); the target, never a Cargo feature, chooses between
//! them.
#![doc(test(attr(deny(warnings))))]
// NOTE: the Rust reference, "The recursion_limit attribute": a page's nested view types
// pass the default query depth of 128 when a test build lays them out.
#![recursion_limit = "256"]

pub mod app;
#[cfg(not(target_arch = "wasm32"))]
pub mod cli;
#[cfg(not(target_arch = "wasm32"))]
pub mod command;
#[cfg(not(target_arch = "wasm32"))]
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod document;
pub mod locale;
pub mod model;
#[cfg(not(target_arch = "wasm32"))]
pub mod oidc;
pub mod pages;
pub mod paths;
#[cfg(not(target_arch = "wasm32"))]
pub mod pix;
#[cfg(not(target_arch = "wasm32"))]
pub mod posts;
pub mod preferences;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;
#[cfg(not(target_arch = "wasm32"))]
pub mod session;

/// Hydrates the page the server drew, whole, and draws every page after it
/// in the browser (FerroUI `docs/consoles.md`, "How a console builds").
///
/// In a debug build a panic before the page has hydrated says its likely
/// cause first; the root element carries `data-ferroui-hydrated` once it has.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    ferrohealth_ui::widget::hydration::explain();
    ferrohealth_ui::widget::hydration::hydrate(app::App);
}
