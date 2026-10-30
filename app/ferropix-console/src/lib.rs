// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! The FerroPIX Console, the data steward's operator console: every page
//! drawn whole on the server with FerroUI, with a backend-for-frontend in
//! front of the FerroPIX server.
//!
//! The console is one more client of the FerroPIX server: it reaches the
//! server through [`pix`], server to server, with the signed-in steward's
//! own access token, and keeps no patient data of its own. It holds the
//! steward's sign-in session on the server ([`session`], [`oidc`]), so the
//! browser carries an opaque cookie and never a token. [`app`] draws each
//! page as one document in the FerroUI shell, with the pages of [`pages`]
//! in the steward's preferences ([`preferences`]) and the request's
//! [`locale`], from the [`visit::Visit`] the server read; [`server`] serves
//! it with FerroUI's assets, the health, sign-in, preference and post
//! routes under the browser security headers, [`config`] reads the TOML
//! configuration, and [`cli`] and [`command`] run the binary over the
//! subcommands and the probe of `ferrohealth-server`. No specification
//! governs the console: our own design (`docs/architecture.md` §12).
#![doc(test(attr(deny(warnings))))]

pub mod app;
pub mod cli;
pub mod command;
pub mod config;
pub mod document;
pub mod locale;
pub mod markup;
pub mod model;
pub mod oidc;
pub mod pages;
pub mod paths;
pub mod pix;
pub mod posts;
pub mod preferences;
pub mod server;
pub mod session;
pub mod visit;
