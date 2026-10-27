// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FerroPIX Console, the data steward's operator console: a Leptos app
//! the server renders whole in islands mode, with a backend-for-frontend in
//! front of the FerroPIX server.
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
//! `wasm32-unknown-unknown`, is the entry that hydrates the islands the
//! server drew, every one of them FerroUI's; the target, never a Cargo
//! feature, chooses between them.
#![doc(test(attr(deny(warnings))))]
// NOTE: the Rust reference, "The recursion_limit attribute": a page's nested view types
// pass the default query depth of 128 when a test build lays them out.
#![recursion_limit = "256"]

#[cfg(not(target_arch = "wasm32"))]
pub mod app;
#[cfg(not(target_arch = "wasm32"))]
pub mod cli;
#[cfg(not(target_arch = "wasm32"))]
pub mod command;
#[cfg(not(target_arch = "wasm32"))]
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod document;
#[cfg(not(target_arch = "wasm32"))]
pub mod locale;
#[cfg(not(target_arch = "wasm32"))]
pub mod oidc;
#[cfg(not(target_arch = "wasm32"))]
pub mod pages;
#[cfg(not(target_arch = "wasm32"))]
pub mod pix;
#[cfg(not(target_arch = "wasm32"))]
pub mod posts;
#[cfg(not(target_arch = "wasm32"))]
pub mod preferences;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;
#[cfg(not(target_arch = "wasm32"))]
pub mod session;

// NOTE: the Leptos book, "Guide: Islands": each island is a WebAssembly export of the
// crate that defines it, so the browser half links FerroUI for every island it draws.
#[cfg(target_arch = "wasm32")]
use ferrohealth_ui as _;

/// Hydrates the islands of the page the server rendered. Each island takes
/// the words it draws as props, so the browser builds no locale.
///
/// It then marks the root element `data-hydrated`, which the browser
/// journeys wait for, so no journey acts on a control whose island has not
/// run.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_islands();
    if let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
        && let Err(error) = root.set_attribute("data-hydrated", "")
    {
        leptos::logging::warn!("the browser refused the hydrated mark: {error:?}");
    }
}
