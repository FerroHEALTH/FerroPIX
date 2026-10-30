---
name: ferroui-lockstep
description: "The operator console (owner 2026-10-21, the fifth FerroUI console) is built on FerroUI (ferrohealth-ui), pinned exactly, in the library's FerroPIX brand, and moves in lockstep with every other console on each FerroUI release; a gap is filed and built in FerroUI (owner 2026-10-18)"
metadata:
  type: project
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

FerroPIX gets an operator console (owner, 2026-10-21): `app/ferropix-console`
(named so by the family rollout, FerroHEALTH/FerroUI#161), its own binary and
image, the fifth FerroUI console beside FerroBRIDGE, FerroEHR, FerroFED and
FerroTERM, designed in `docs/architecture.md` §12 and built new on FerroUI
0.3.2 in v0.0.3 (#119). Every FerroHEALTH operator console is built on one shared
component library, FerroHEALTH/FerroUI (the crate `ferrohealth-ui`), pinned
exactly as a Cargo dependency (owner, 2026-10-18: "i want to have an uniform
ui compoanats libary for all the different UI's so all the products look and
feel the same"), and the owner's bar for this one is the best user interface
and experience in the family.

**How to apply:**

- From FerroUI 0.6.0 it is drawn on the server like every console: each
  page one whole document an axum handler draws with FerroUI's Askama
  components, FerroUI's small script modules adding behaviour on top, no
  Leptos and no WebAssembly, every page complete with the modules refused
  (owner rulings of 2026-10-29 on FerroUI #510; moved under #388; this
  supersedes [[reactive-console-ui]]). It takes every component from
  FerroUI, and draws in the library's FerroPIX brand theme
  (Plum & Iron, `Brand::Pix`, filed on FerroUI), never by forking a
  component or keeping a theme of its own.
- A component or a pattern FerroUI lacks is filed on FerroUI and built
  there, never as a local copy; the FerroPIX issue that needs it says so in
  a sentence, since no native edge crosses repositories.
- Every FerroUI release moves every console in lockstep: a pin-bump pull
  request here beside the ones in the other products, merged together, with
  screenshots at 1440 and 390 px in light and dark reviewed before merge.
