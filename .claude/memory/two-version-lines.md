---
name: two-version-lines
description: "The product version is the workspace version and the release tag; each published crate keeps its own version line, never the product's; ihe-iti continues from 0.0.39 and nl-generic-functions from 0.0.25 (owner 2026-10-20, the FerroBRIDGE model)"
metadata:
  type: feedback
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

FerroPIX has two version lines, as FerroBRIDGE has. The product version is
the `[workspace.package]` version, the release tag and the changelog
section; v0.0.2 is the release that moves the crates. Each published crate
keeps its own `version` in its manifest: `ihe-iti` continues FerroFED's line
from 0.0.39 and `nl-generic-functions` from 0.0.25, though crates.io held
only their 0.0.0 reservations.

**Why:** the owner, 2026-10-20 (FerroHEALTH/FerroFED#367): the move releases
as v0.0.2 while the crates continue above FerroFED's local versions, which
one shared line cannot hold.

**How to apply:** a change to a crate's packaged content bumps that crate's
own version with the lock; a release pull request moves only the product
version and the changelog. `scripts/checks/crate-version-guard.sh` holds
both lines; `.claude/rules/crates-publishing.md` is the rule.
