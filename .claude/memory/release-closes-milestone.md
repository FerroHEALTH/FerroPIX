---
name: release-closes-milestone
description: "The release workflow refuses a tag whose milestone is missing or not empty, and closes the milestone after every leg succeeds; after a manual or recovery release, check it is closed (owner 2026-10-19, FerroSMART #25)"
metadata:
  type: feedback
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

A published release closes its milestone: `release.yml` refuses a `vX.Y.Z`
tag in `plan` when the milestone titled exactly as the tag is missing or
holds an open item, and its last job closes that milestone once every leg has
succeeded (`scripts/release/milestone.sh`). While no crate is published, the
two crate legs are skipped and count as done (`SKIPPABLE_LEGS`).

**Why:** FerroSMART v0.1.0 was tagged and published on 2026-10-19 while its
milestone stayed open, and the owner asked that this be impossible in every
product.

**How to apply:** the workflow enforces it. A release is cut when its
milestone holds no open item: a release pull request moves the version and
the changelog, it merges, and the session signs and pushes the tag
(`git tag -s vX.Y.Z -m vX.Y.Z`, `git push origin vX.Y.Z`) on the merge
commit; the tag is never handed to the owner. After any manual or recovery
release, check the milestone is closed, and run
`GITHUB_REPOSITORY=FerroHEALTH/FerroPIX bash scripts/release/milestone.sh close vX.Y.Z`
when it is not.
