<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# ferropix-console

The operator console. The Leptos discipline is `.claude/rules/leptos-ui.md`,
which loads for every file here and leads with FerroUI; this file is what is
particular to the crate.

- **`main.rs` never grows.** It calls `command::run(std::env::args())`; the
  tests drive `run`, `Config::from_sources`, `router` and `Pix`.
- **`crate::pix` owns every call to the FerroPIX server**, and a page draws
  only what a call brought back (`pages::server_state` until the operator
  API exists).
- **Every configuration default lives inline in its struct's `Default`
  impl**, with container-level `#[serde(default, deny_unknown_fields)]`. A
  refusal names the key, never the value; a secret goes through
  `config::secret::resolve`.
- **A page is a synchronous server component** drawn from the
  `PageContext` the `gate` middleware read; a route with an id checks it
  before anything else.
- **A new route joins the captures** (`tests/it/captures.rs`), the page
  tests (`tests/it/pages.rs`) and the journeys' `SCREENS`
  (`tools/ferropix-testkit/tests/it/journeys/mod.rs`, held by a guard), and is set beside its mockup in FerroUI's
  `design/consoles/ferropix/` at 1440 and 390 px, light and dark.
