---
name: reactive-console-ui
description: "Superseded on 2026-10-29 by the owner's rulings on FerroUI #510: every console is drawn on the server, a navigation is one document load, an in-page change is a FerroUI module, and no console runs WebAssembly; was: no full page reload for an interaction (owner, 2026-10-25)"
metadata:
  type: feedback
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

**Superseded.** The owner's rulings of 2026-10-29 on FerroUI #510 replace
this: FerroUI 0.6.0 draws every component on the server, a link or a form
loads one complete document the console's server drew, an in-page change (a
tab, a menu, a dialog, a side panel) is a FerroUI module toggling what the
server drew, and no console runs Leptos or WebAssembly. FerroPIX moved
under #388. The ruling below is kept as the record of what it replaced.

The owner ruled on 2026-10-25 that every FerroHEALTH console stays reactive:
a link, a form or an action never reloads the whole document. WebAssembly is
the normal path; a page that works without it is the fallback, never the
design target.

**Why:** FerroUI #196 (PR #223, 2026-10-22) moved the consoles to islands
mode on bundle size and time-to-interactive alone and made every action a
plain form post, so each click became a full page load, which the owner
found slow and unacceptable. FerroUI #410 restores client navigation.

**How to apply:** user experience outranks bundle bytes. Judge a UI change
by the interaction it gives, measure navigations as well as first load, and
never land a design whose every interaction reloads the page.
