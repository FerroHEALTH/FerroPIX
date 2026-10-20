---
name: ferroui-lockstep
description: "If FerroPIX ever gets an operator console, it is built on FerroUI (ferrohealth-ui), pinned exactly, and moves in lockstep with every other console on each FerroUI release (owner 2026-10-18)"
metadata:
  type: project
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

FerroPIX has no user interface today. Every FerroHEALTH operator console is
built on one shared component library, FerroHEALTH/FerroUI (the crate
`ferrohealth-ui`), pinned exactly as a Cargo dependency (owner, 2026-10-18:
"i want to have an uniform ui compoanats libary for all the different UI's so
all the products look and feel the same").

**How to apply, if a console is ever added:**

- It is server rendered and hydrated like the other consoles, takes its
  components from FerroUI, and sets only its own colours through the role
  tokens (`assets/brand/tokens.css`), never by forking a component.
- A component FerroUI lacks is filed on FerroUI and built there, never as a
  local copy.
- Every FerroUI release moves every console in lockstep: a pin-bump pull
  request here beside the ones in the other products, merged together, with
  screenshots in light and dark reviewed before merge.
