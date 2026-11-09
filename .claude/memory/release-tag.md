---
name: release-tag
description: "A release is a signed vX.Y.Z tag on the merge commit on main, pushed from the session; the session approves the crates-io gate itself (owner 2026-11-03); a bad cut ships forward as a new patch"
metadata:
  type: project
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

The release pull request bumps the product version (the workspace version)
and the changelog section; each crate's own version moves in the pull
requests that change it ([[two-version-lines]]). After it merges, the tag
`vX.Y.Z` is created signed (`git tag -s`) on that merge commit and pushed
from the session; `release.yml` builds the draft, attests each listed
`.crate`, publishes the release, and its `crates` leg pauses on the `crates-io` environment, which the session approves itself.

**How to apply:** the session runs the whole cut: it arms the release PR with `gh pr merge <n> --auto`, signs and pushes the tag, and approves the `crates-io` deployment with `gh api -X POST repos/<repo>/actions/runs/<run>/pending_deployments` (owner, 2026-10-27 and 2026-11-03: "you can approve crates.io and also tag releases fully autonomously, without me asking anything"). Never move or delete a pushed tag (the `release-tags`
ruleset and immutable releases refuse it); a bad cut is fixed by a new patch
version. A worker under a coordinating session reports the merge commit and leaves the tag to the coordinator.
