---
paths: ["app/ferropix-console/**"]
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Leptos console rules (`app/ferropix-console`)

**FerroUI leads the UI.** The console draws with `ferrohealth-ui`, pinned
exactly in the root `[workspace.dependencies]` at the version the
FerroUI row of `docs/VERSIONS.md` names, and the FerroUI repository
(FerroHEALTH/FerroUI) is the source for its components, design, tokens,
icons, accessibility bar, interaction contract, words and quality control.
Read these first, at the git tag that matches the pin (the links below are
the `v0.5.2` copies; read the tag of the pin when it moves):

- [`docs/consoles.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.5.2/docs/consoles.md):
  the consumer guide, what the library owns and what a console owns, how a
  console builds and how it watches the session's end.
- [`.claude/rules/leptos-ui.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.5.2/.claude/rules/leptos-ui.md):
  the component rules of every FerroHEALTH console. The section numbers this
  file cites (FerroUI §N) are those of the `v0.5.2` copy.
- [`crates/ferrohealth-ui/CLAUDE.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.5.2/crates/ferrohealth-ui/CLAUDE.md):
  the interaction contract and the rendering mode.
- [`design/COMPONENTS.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.5.2/design/COMPONENTS.md)
  and [`design/consoles/ferropix/`](https://github.com/FerroHEALTH/FerroUI/tree/v0.5.2/design/consoles/ferropix):
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

- **The whole page hydrates, two halves chosen by target** (FerroUI
  `docs/consoles.md`, "How a console builds", and FerroUI §15). The server
  draws the first page; the `wasm32-unknown-unknown` build, `hydrate()`
  over FerroUI's `widget::hydration::hydrate`, takes the whole tree over and
  draws every page after it in the browser, so a link, a tab or a search
  keeps the document. The host build is the server half (`leptos/ssr`,
  axum, the session and the client of the FerroPIX server, each
  `#[cfg(not(target_arch = "wasm32"))]`); the page tree (`app`, `pages`,
  `model`, `locale`, `preferences`, `paths`) compiles for both. Neither
  half turns on the `leptos` feature `islands`, and the crate has no `ssr`
  or `hydrate` feature, so a workspace `--all-features` build never turns
  both on.
- **The pages sit in four lazy route groups** (`app::routes`: `Work`,
  `Patients`, `Registry` and `Aside`), each a chunk of its own under `--split`, loaded
  the first time the browser draws one of its pages. The shell is FerroUI's,
  drawn once around them and following the router
  (`widget::nav::follow_path`).
- **Every page is drawn from its `PageContext`.** The `gate` middleware of
  `server::context` holds the session, and `page_for` calls the FerroPIX
  server once before the first page renders (the steward's name, the
  outcome of the liveness route, on About the `CapabilityStatement`); the
  context reaches the hydrating page serialized. After a navigation the
  page is read through the `page` server function (`app::page_now`), and a
  form drawn in place (`app::in_page`: a search, a trail filter, a reveal,
  a domain's form) posts through the `post` server function
  (`app::post_now`). Both reads are a resource
  taken in the page's `Transition` and in an effect, never in a memo
  (#323), and the old page stays until the new one comes.
- **The session's end is watched** (FerroUI `docs/consoles.md`, "The
  session's end"). The shell of a signed-in page draws FerroUI's
  `SessionWatch` over the session route (`paths::SESSION`,
  `session::route`: `GET` reads what the session has left, `POST` extends
  it), from what the session had left as the server drew the first page; a
  session the watch or a page read finds gone sends every tab to
  `/sign-in/expired` with the page's address as its return (#322).
- **Every widget is FerroUI's** (FerroUI §1, "The interaction contract
  and the three parts"). The console writes a component of its own only
  where a widget shares state with the console's code, justified against
  the bundle bytes it adds (§4); the posts drawn in place and the page
  reads above are the console's own because they share the page context.
- **Every important action is a link or a form** (FerroUI §1, item 3): a
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
  the test provider in `tests/it/sign_in.rs`, `refresh.rs` and
  `sign_out.rs`.
- **Every request that is not a safe method comes from the console's own
  pages** (`server`'s `same_origin_check`, over `ferrohealth-server`'s
  `same_origin`), and every page and route that acts needs a live session
  (`server::context`'s `gate`), else it goes to the sign-in page.
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
  file. `scripts/checks/stray-literals.sh` (CI job `stray-literals`, FerroUI
  §13) fails a literal under `src/` that reads as words; one no reader sees
  as text (an HTTP request line, a header value) is allowed with its reason
  in `scripts/checks/stray-literals.allow`, which also skips the modules that
  write only to the terminal. The pseudo-locale, `qps` (`locale::pseudo`),
  draws every console and library word accented and two fifths longer; only
  a language cookie naming it chooses it, and the pseudo-locale journey
  walks every screen in it, failing on a word it left unstretched and on a
  page that scrolls sideways at 1440 or 390 px.

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
  them as the design renders are drawn. The committed captures are #98.
- **The browser journeys** (`tools/ferropix-testkit/tests/it/journeys/`,
  behind `FERROPIX_JOURNEYS`, CI job `journeys`) drive the console in
  headless Chrome for Testing over the test OpenID Provider and a stand-in
  server: sign-in and sign-out, the preferences, the pseudo-locale, and the
  WCAG 2.2 AA battery over every screen's refused state in both themes
  (text contrast, control edges, the keyboard walk with its focus rings,
  the markup, the reflow at 320 px), with a self-test that a wrong theme
  fails, and a link and a patient search that keep the document. Every
  wait waits for the `data-ferroui-hydrated` mark FerroUI's
  `widget::hydration::hydrate` sets on `<html>` once the page and its lazy
  route have hydrated, and a press for the page it loads or draws in place. `journeys::SCREENS` names
  one address per route, held by a guard in the ordinary suite; a screen's
  filled state joins the battery when the screen lands. Locally:
  `scripts/release/journey-tools.sh target/journey-tools`, the site bundle,
  then `FERROPIX_JOURNEYS=1 cargo nextest run -p ferropix-testkit -E
  'test(/^journeys::/)'`.
- **The bundle budget**, over the entry WebAssembly of
  `cargo leptos build --release --split`, brotli `-q 11`:

  | Measure | Budget |
  |---|---|
  | Entry WebAssembly, brotli-compressed | 598016 bytes |

  Measured 2026-10-26 at 477374 bytes on the move to FerroUI 0.5 (#214,
  cargo-leptos 0.3.11, wasm-bindgen 0.2.129, Rust 1.99.1), with the pages
  in four lazy route groups (work, patients, registry, aside) whose chunks
  hold 511934 more; the budget is a quarter over it, rounded up to whole
  KiB. Measured again 2026-10-27 at 474930 bytes on FerroUI 0.5.2, whose
  `widget::hydration::hydrate` entry replaced `hydrate_lazy`, with chunks
  of 507357, so the budget stands, 26 per cent over that build. Whole-page hydration
  puts the renderer, the reactive graph, the router, the shell, the page
  context's serde and every data segment in the entry, since `--split`
  moves code alone. Under islands it measured 295172 (2026-10-23). The `console-wasm` job of `ci.yml`
  builds the bundle with `scripts/release/console-site.sh --release` and
  fails over the budget (`scripts/checks/console-bundle.sh`, which reads it
  from the table above and writes the sizes to the job summary).
