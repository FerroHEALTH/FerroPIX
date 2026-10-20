---
name: release-tag
description: "A release is a signed vX.Y.Z tag on the merge commit on main, pushed from the session; the crates leg waits on the crates-io environment's required reviewer, the owner; a bad cut ships forward as a new patch"
metadata:
  type: project
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

The release pull request bumps the workspace version and the changelog
section (and the lock, once a crate is published). After it merges, the tag
`vX.Y.Z` is created signed (`git tag -s`) on that merge commit and pushed
from the session; `release.yml` builds the draft, attests each listed
`.crate`, publishes the release, and its `crates` leg pauses on the
`crates-io` environment until the owner approves.

**How to apply:** never move or delete a pushed tag (the `release-tags`
ruleset and immutable releases refuse it); a bad cut is fixed by a new patch
version. Tell the owner when the run waits on the approval.
