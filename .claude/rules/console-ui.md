---
paths: ["app/ferropix-console/**"]
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Console rules (`app/ferropix-console`)

**FerroUI leads the UI.** The console draws with `ferrohealth-ui`, pinned
exactly in the root `[workspace.dependencies]` at the version the
FerroUI row of `docs/VERSIONS.md` names, and the FerroUI repository
(FerroHEALTH/FerroUI) is the source for its components, design, tokens,
icons, script modules, accessibility bar, interaction contract, words and
quality control. Read these first, at the git tag that matches the pin (the
links below are the `v0.6.0` copies; read the tag of the pin when it moves):

- [`docs/consoles.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.6.0/docs/consoles.md):
  the consumer guide, what the library owns and what a console owns, how a
  console builds, the top bar's search and the session's end.
- [`docs/migration-0.6.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.6.0/docs/migration-0.6.md):
  what replaced each part of the hydrated 0.5 console.
- [`.claude/rules/console-ui.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.6.0/.claude/rules/console-ui.md):
  the component rules of every FerroHEALTH console. The section numbers this
  file cites (FerroUI §N) are those of the `v0.6.0` copy.
- [`crates/ferrohealth-ui/CLAUDE.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.6.0/crates/ferrohealth-ui/CLAUDE.md):
  the interaction contract and the rendering mode.
- [`design/COMPONENTS.md`](https://github.com/FerroHEALTH/FerroUI/blob/v0.6.0/design/COMPONENTS.md)
  and [`design/consoles/ferropix/`](https://github.com/FerroHEALTH/FerroUI/tree/v0.6.0/design/consoles/ferropix):
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
`COMPONENTS.md`, the gallery, the crate's public API at the pin and
FerroUI's open issues; a filed issue names the existing parts it considered
and why each does not fit.

This file keeps only what is particular to this console. Everything else is
FerroUI's. No specification governs the rendering mode: FerroUI's own
design (owner rulings of 2026-10-29 on FerroHEALTH/FerroUI#510).

## 1. The shape

- **Every page is one document the server draws whole** (FerroUI
  `docs/consoles.md`, "How a console builds"). An axum handler reads what
  the page needs, then `app::page` draws it through FerroUI's
  `view::shell::document`, which writes `<html lang>`, the preference
  attributes, FerroUI's stylesheet, the FerroPIX theme and the one loader
  script; a link or a form loads the next document. The console writes no
  script and no Leptos, and builds with `cargo build` alone: there is no
  `wasm32` target, no `cargo-leptos` and no site bundle.
- **FerroUI's assets are FerroUI's router.** `server::router` nests
  `serve::axum_router::router()` at `serve::PREFIX`, which serves the
  stylesheet, the brand themes and the script modules under
  `/ferrohealth-ui/<version>/` with a one-year immutable cache; the console
  compresses them (never a document) and serves only its brand marks and
  favicon itself.
- **Every page is drawn from its `Visit`** (`crate::visit`): the
  `PageContext` the `gate` middleware of `server::context` read for the
  request, the locale it names and the route's parameters. `page_for`
  calls the FerroPIX server before the page is drawn (the steward's name,
  the outcome of the liveness route, on About the `CapabilityStatement`, and
  the page's operator API document), so a page reads nothing while it is
  drawn. `app::route` names the page an address answers; a page is a plain
  function from the `Visit` to a `pages::Drawn` (its title, its body, its
  docked bar and its status), with the console's own markup written as
  FerroUI's gallery writes it (`crate::markup`: markup made from escaped
  parts, `t` for text).
- **A form posts to its page's address and is answered with that page**
  (FerroUI `docs/consoles.md`, "An action that answers on its page"):
  `posts::post` reads the form, asks the FerroPIX server, and draws the page
  again with the answer in its context, `200`, so a search, a filter, a
  reveal or a decision never puts what was typed in an address. A
  decision that leads on (a link, a merge) answers `303` to the next page.
- **The session's end is watched** (FerroUI `docs/consoles.md`, "The
  session's end"). The shell of a signed-in page draws FerroUI's
  `view::session::session_watch` over the session route (`paths::SESSION`,
  `session::route`: `GET` reads what the session has left, `POST` extends
  it), from what the session had left as the server drew the page; the
  sign-in pages for a sign-out and an ended session draw `session_ended`,
  and a request with no live session is sent to `/sign-in/expired`.
- **Every widget is FerroUI's** (FerroUI §1). An in-page change (a side
  panel, a dialog, a menu, the work-list's focus, the search panel, the
  reveal's countdown, the preference that posts on a change) is FerroUI's
  module over what the server drew.
- **Every important action is a link or a form** (FerroUI §1): a search, a
  preference, Try again, a decision, sign-out and the sidebar's rail all
  work with FerroUI's modules refused, which the `no_script` journeys hold.
- **No key the console cannot bind is announced.** A console on 0.6 binds
  no key of its own; the pair's `l`, `n`, `d`, `r`, `[` and `]` and the
  queue's `o` and `d` come back with #390 once FerroUI binds them
  (FerroHEALTH/FerroUI#526, FerroHEALTH/FerroUI#530).

## 2. The server boundary

- **The console is a client of the FerroPIX server and nothing else.**
  `crate::pix` is the one place it calls the server, with the signed-in
  steward's own token and an `X-Request-Id` it mints. It links no workspace
  crate beside `ferropix-operator-api`, the operator API's contract, and no
  crate of the Leptos framework on any target; `tests/it/boundary.rs` walks
  the resolved graph and fails on either. A page that needs what the
  server does not serve gets an operator API issue on the server first,
  never a call the architecture does not name.
- **A page never draws data the server did not send.** While the server
  cannot give a page what it reads, the page draws its refused state
  (`pages::server_state`): the refusal of the call with Try again, its time
  and its technical detail, or the notice that names the operator API route
  the page reads.
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
- **The headers are tested in-process** (`tests/it/server.rs`): the
  Content-Security-Policy with `script-src 'self'` and `style-src 'self'`
  and no nonce (`server::policy`, written over `ferrohealth-server`'s
  hydrated policy until #389), `nosniff`, `DENY`, `no-referrer`, `no-store`
  on documents, FerroUI's immutable cache on its assets, and only the
  assets compressed.

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
  §0, §13), looked up through `crate::locale` (`Visit::tr`) in the locale the request chose
  (the language preference, else `Accept-Language`, else `[server] locale`),
  with FerroUI's number formatter and the `number-group` and `number-decimal`
  marks set. The build script writes one `Msg` variant per English message,
  documented by the `#` comment right above it, and fails the build on a
  message without one. A test holds every catalog to `Msg`, and a
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

## 4. Preferences, captures and the journeys

- **Each preference is its own cookie** (FerroUI family rule 9), named
  `ferropix_console_<name>` under `__Host-` when the cookies are `Secure`:
  FerroUI's six and the console's own two, the reveal duration (10, 15 or 30
  seconds, the server capping every reveal at 30) and hide on leave, inside
  Appearance. The theme is set only in the user menu and Your preferences;
  the top bar carries no theme toggle (rule 5).
- **Every page is set beside its mockup at 1440 and 390 px, light and dark**,
  before a change to it merges. `tests/it/captures.rs`, behind
  `FERROPIX_CAPTURES` (a directory), writes every page as a browser receives
  it with FerroUI's stylesheet, theme and loader beside it; FerroUI's
  `design/tools/render.py` capture draws them as the design renders are
  drawn. The book's screenshots are the testkit's `screenshots` journey,
  behind `FERROPIX_SCREENSHOTS`, looked at before they are committed.
- **The browser journeys** (`tools/ferropix-testkit/tests/it/journeys/`,
  behind `FERROPIX_JOURNEYS`, CI job `journeys`) drive the console in
  headless Chrome for Testing over the test OpenID Provider and a stand-in
  server: sign-in and sign-out, the preferences, the pseudo-locale, the
  steward's work, and the WCAG 2.2 AA battery over every screen's refused
  and filled state in both themes (text contrast, control edges, the
  keyboard walk with its focus rings, the markup, the reflow at 320 px),
  with a self-test that a wrong theme fails. `navigation` holds that a link
  and a search load a document and a side panel loads none; `no_script`
  refuses FerroUI's modules (`/ferrohealth-ui/<version>/scripts/`) and
  holds every screen to its floor (its content, a link or a form behind
  every action, no control that works only with a module, the reflow at
  320 px), as the steward's work runs both ways. Every wait waits for the
  loader's mark on `<html>` (`ferrohealth_ui::script::MARK`), or, with the
  modules refused, for the document alone, and a press for the page it
  loads. `journeys::SCREENS` names one address per route, held by a guard
  in the ordinary suite. Locally:
  `scripts/release/journey-tools.sh target/journey-tools`, then
  `FERROPIX_JOURNEYS=1 cargo nextest run -p ferropix-testkit -E
  'test(/^journeys::/)'`.
- **No bundle of its own.** FerroUI holds the script budget of every module
  (16 KiB each, 160 KiB for all, FerroUI §12); the console adds no script,
  so it has no budget of its own.
