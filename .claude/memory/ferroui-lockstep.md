---
name: ferroui-lockstep
description: "The operator console (owner 2026-10-21, the fifth FerroUI console) is built on FerroUI (ferrohealth-ui), pinned exactly, in the library's FerroPIX brand, and moves in lockstep with every other console on each FerroUI release; a gap is filed and built in FerroUI (owner 2026-10-18)"
metadata:
  type: project
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

FerroPIX gets an operator console (owner, 2026-10-21): `app/ferropix-viewer`,
its own binary and image, the fifth FerroUI console beside FerroBRIDGE,
FerroEHR, FerroFED and FerroTERM, designed in `docs/architecture.md` §12 and
built in v0.0.4. Every FerroHEALTH operator console is built on one shared
component library, FerroHEALTH/FerroUI (the crate `ferrohealth-ui`), pinned
exactly as a Cargo dependency (owner, 2026-10-18: "i want to have an uniform
ui compoanats libary for all the different UI's so all the products look and
feel the same"), and the owner's bar for this one is the best user interface
and experience in the family.

**How to apply:**

- It is server rendered and hydrated like the other consoles, takes every
  component from FerroUI, and draws in the library's FerroPIX brand theme
  (Plum & Iron, `Brand::Pix`, filed on FerroUI), never by forking a
  component or keeping a theme of its own.
- A component or a pattern FerroUI lacks is filed on FerroUI and built
  there, never as a local copy; the FerroPIX issue that needs it says so in
  a sentence, since no native edge crosses repositories.
- Every FerroUI release moves every console in lockstep: a pin-bump pull
  request here beside the ones in the other products, merged together, with
  screenshots at 1440 and 390 px in light and dark reviewed before merge.
