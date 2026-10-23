---
paths: ["app/ferropix-console/**"]
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Leptos console rules (`app/ferropix-console`)

**FerroUI leads the UI.** The console draws with `ferrohealth-ui`, pinned
exactly in the root `[workspace.dependencies]` (today `=0.3.5`, a row of
`docs/VERSIONS.md`), and the FerroUI repository (FerroHEALTH/FerroUI) is the
source for its components, design, tokens, icons, accessibility bar,
interaction contract, words and quality control. Read these first, at the git
tag that matches the pin (today `v0.3.5`), and on `main` only while the
console moves to the next release:

- [`docs/consoles.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.3.5/docs/consoles.md):
  the consumer guide, what the library owns and what a console owns.
- [`.claude/rules/leptos-ui.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.3.5/.claude/rules/leptos-ui.md):
  the component rules of every FerroHEALTH console. The section numbers this
  file cites (FerroUI §N) are those of the `v0.3.5` copy.
- [`crates/ferrohealth-ui/CLAUDE.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.3.5/crates/ferrohealth-ui/CLAUDE.md):
  the interaction contract and islands mode.
- [`design/COMPONENTS.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.3.5/design/COMPONENTS.md)
  and [`design/consoles/ferropix/`](https://github.com/FerroHEALTH/FerroUI/tree/v0.3.5/design/consoles/ferropix):
  every component by its ID, the family rules, and this console's pages as
  reviewed. FerroUI's `design/` is the only design source; a design folder
  anywhere else is stale and is never synced from.

A component this console needs and the library lacks, one that behaves
wrongly, or one that could be better is an issue filed in FerroHEALTH/FerroUI
against that component (its ID in `COMPONENTS.md`, what is missing, this
console as the one that needs it), named in a sentence on the FerroPIX issue
that waits on it: never a local copy, a wrapper, a shadow component, a
stylesheet override or a hand-written script here. Until the FerroUI release
lands, the console uses the component as it is. Before filing, check
`COMPONENTS.md`, the gallery and the crate's public API at the pin; a filed
issue names the existing parts it considered and why each does not fit.

This file keeps only what is particular to this console. Everything else is
FerroUI's, and the oracle for a Leptos question is the Leptos book
(<https://github.com/leptos-rs/book>, `main`, Leptos 0.8) and the 0.8 source
the workspace resolves, never memory.

## 1. The shape

- **Islands mode, two halves chosen by target** (FerroUI §1, §2, §15). The
  host build is the server half: every module but the hydrate entry is
  `#[cfg(not(target_arch = "wasm32"))]`, and the server draws every page
  whole. The `wasm32-unknown-unknown` build is the browser entry alone,
  `hydrate()` over `leptos::mount::hydrate_islands()`, linking FerroUI for
  its islands. The crate has no `ssr` or `hydrate` feature, so a workspace
  `--all-features` build never turns both on.
- **Every island is FerroUI's.** The console writes no `#[island]` of its own
  until a widget shares state with the console's code (FerroUI §1); one that
  does is justified against the bundle bytes it adds (§4).
- **A page reads nothing asynchronously while it renders.** The `gate`
  middleware of `server.rs` holds the session and calls the FerroPIX server
  once before the render, and hands the page a `PageContext` (the steward's
  name, the outcome of the liveness route, on About the
  `CapabilityStatement`). A page is a synchronous server component drawn
  from it, so no resource is serialized and no streaming script runs.
- **Every important action is a link or a form** (FerroUI §1 layer 3): a
  search, a preference, Try again, sign-out and the sidebar's rail all work
  with the WebAssembly withheld.
- **The bundle name is fixed at compile time.** `.cargo/config.toml` sets
  `LEPTOS_OUTPUT_NAME` to the crate's `output-name`, so a plain cargo build
  renders the page cargo-leptos's bundle hydrates; the server test holds it.

## 2. The server boundary

- **The console is a client of the FerroPIX server and nothing else.**
  `crate::pix` is the one place it calls the server, with the signed-in
  steward's own token and an `X-Request-Id` it mints. It links no workspace
  crate beside `ferropix-operator-api`, the operator API's contract once #90
  adds it; `tests/it/boundary.rs` walks the resolved graph of both halves and
  fails on another. A page that needs what the server does not serve gets an
  operator API issue on the server first, never a call the architecture does
  not name.
- **A page never draws data the server did not send.** While the server
  cannot give a page what it reads, the page draws its refused state
  (`pages::server_state`): the refusal of the call with Try again, its time
  and its technical detail, or the notice that names the operator API route
  the page reads. A `TODO(#N)` beside each names the issue that fills it.
- **The session is the server's** (`session`, `oidc`): authorization code
  with PKCE, the ID Token checked against the provider's keys, issuer,
  audience and nonce, one opaque `__Host-` cookie in the browser, a token, a
  code, a verifier or a client secret never in a page, a cookie, a `Debug`
  rendering or a log line. Sign-in, refresh and sign-out are tested against
  the test provider in `tests/it/provider.rs`.
- **Every request that is not a safe method comes from the console's own
  pages** (`server::same_origin_only`), and every page and route that acts
  needs a live session (`server::gate`), else it goes to the sign-in page.
- **The headers are tested in-process** (`tests/it/server.rs`): the CSP
  nonce with `style-src 'self'`, `nosniff`, `DENY`, `no-referrer`, `no-store`
  on documents, and only the site bundle compressed.

## 3. Identifiers and words

- **No identifier in an address, a log line or a fixture** but the person id
  FerroPIX mints (`docs/architecture.md` §12.1, FerroFED's identifier-hygiene
  rule). A search posts, its redirect carries no field, the identifier field
  is a masked input, and a route id is checked as a UUID or a short name
  before the server is asked (`pages::screens::is_uuid`, `is_name`), so a
  business identifier in an address is the not-found page. FerroPIX is
  international: an identifier is a domain and a value, never a BSN by
  default, and every synthetic example fails its scheme's check (a nine-digit
  number fails the eleven-proof).
- **Every word is a message of `locales/en/ferropix-console.ftl`** (FerroUI
  §0, §13), looked up through `crate::locale` in the locale the request chose
  (the language preference, else `Accept-Language`, else `[server] locale`),
  with FerroUI's number formatter and the `number-group` and `number-decimal`
  marks set. A test holds the catalog to every message and no other, and a
  throwaway catalog under `tests/locales/` proves a language joins as one
  file.

## 4. Preferences, captures and the bundle

- **Each preference is its own cookie** (FerroUI family rule 9), named
  `ferropix_console_<name>` under `__Host-` when the cookies are `Secure`:
  FerroUI's six and the console's own two, the reveal duration (10, 15 or 30
  seconds, the server capping every reveal at 30) and hide on leave, inside
  Appearance. The theme is set only in the user menu and Your preferences;
  the top bar carries no theme toggle (rule 5).
- **Every page is set beside its mockup at 1440 and 390 px, light and dark**,
  before a change to it merges. `tests/it/captures.rs`, behind
  `FERROPIX_CAPTURES` (a directory), writes every page as a browser receives
  it with its stylesheets; FerroUI's `design/tools/render.py` capture draws
  them as the design renders are drawn. The browser journeys and the
  committed captures are #98.
- **The bundle budget**, over the entry WebAssembly of
  `cargo leptos build --release --split`, brotli `-q 11`:

  | Measure | Budget |
  |---|---|
  | Entry WebAssembly, brotli-compressed | 393216 bytes |

  Measured 2026-10-23 at 295172 bytes (cargo-leptos 0.3.11, wasm-bindgen
  0.2.129, Rust 1.99.1), with FerroUI's lazy date pickers and tree in chunks
  of their own; the budget is a third over it, as FerroUI's gallery keeps
  its own. FerroUI 0.3.3 with its `split` feature (#127) measured 298123, FerroUI 0.3.4 (#131) 300608, and FerroUI 0.3.5 (#137) 301105, with 184363 more in its lazy chunks (142636 in three `split_*` files, 41727 in three `chunk_*` files). The `console` lane that builds the bundle against it in CI is
  #99; until then a change that adds browser code measures it by hand.
