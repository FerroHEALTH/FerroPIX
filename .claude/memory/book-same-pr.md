---
name: book-same-pr
description: "The book at ferropix.eu documents only what the code does, changes in the same PR as the code, names the unbuilt as planned with a tracker link, and never types a moving version; owner 2026-10-21"
metadata:
  type: feedback
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

The documentation site (<https://ferropix.eu/>, `website/`) is kept current
forever, at the standard of the other FerroHEALTH products (owner,
2026-10-21, #44).

**Why:** a book that drifts from the code is a false claim to every reader,
and a link check passes on a page whose every claim is false.

**How to apply:**

- A change to a crate's behaviour or features, the server or a deployment
  file updates its page under `website/book/src` in the same pull request;
  `book-guard.sh` refuses it otherwise, and `no-book` is only for a change
  that alters nothing the book says.
- Unbuilt work, a sibling's wiring to FerroPIX included, is named as
  planned and links its issue or milestone (`docs-claims.sh`).
- Versions are markers (`{{version:ihe-iti}}`) or includes of
  `docs/VERSIONS.md`, never typed.
- A website pull request is looked at in a browser at 1440 and 390 pixels,
  light and dark, before it merges.
- The detail is `.claude/rules/docs-website.md`.
