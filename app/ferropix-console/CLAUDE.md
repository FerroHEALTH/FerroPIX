<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# ferropix-console

The operator console. The console discipline is `.claude/rules/console-ui.md`,
which loads for every file here and leads with FerroUI; this file is what is
particular to the crate.

- **`main.rs` never grows.** It calls `command::run(std::env::args())`; the
  tests drive `run`, `Config::from_sources`, `router` and `Pix`.
- **`crate::pix` owns every call to the FerroPIX server**, and a page draws
  only what a call brought back (`pages::server_state` when it did not).
- **Every configuration default lives inline in its struct's `Default`
  impl**, with container-level `#[serde(default, deny_unknown_fields)]`. A
  refusal names the key, never the value; a secret goes through
  `config::secret::resolve`.
- **A page is a plain function from its `Visit` to a `pages::Drawn`**, drawn
  from the `PageContext` the `gate` middleware read, with FerroUI's
  components and the console's own markup through `crate::markup` (every
  text through `t`); a route with an id checks it before anything else. No
  script, no inline style, no component of the console's own.
- **A new route joins `app::route`, the server's `PAGES`**, the captures
  (`tests/it/captures.rs`), the page tests (`tests/it/pages.rs`) and the
  journeys' `SCREENS`
  (`tools/ferropix-testkit/tests/it/journeys/mod.rs`, held by a guard), and is set beside its mockup in FerroUI's
  `design/consoles/ferropix/` at 1440 and 390 px, light and dark.
