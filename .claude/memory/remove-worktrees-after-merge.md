---
name: remove-worktrees-after-merge
description: "Every checkout lives under .claude/worktrees/; remove a worktree, its branches and its build folder as soon as its PR merges, in every family repository; run cargo clean where a target/ remains (owner 2026-10-15, 2026-10-19)"
metadata:
  type: feedback
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

Branch work happens in a worktree under `.claude/worktrees/<name>`
(`git worktree add .claude/worktrees/<name> -b <branch> origin/main`), never
a sibling folder beside the project. Remove it, and its local branches, as
soon as the PR carrying its work is merged, and run `git fetch --prune`.

**Why:** the owner found leftover worktrees and asked (2026-10-15) that they
"never hang there", and found the disk at 1.1 TB used on 2026-10-19 ("keep
cleaning the cargo and things"). The rule holds in every FerroHEALTH
repository, for the orchestrator and for every worker brief.

**How to apply:** on each merge: `git pull --ff-only` in the main checkout,
`cargo clean` in the worktree if it built anything, `git worktree remove
.claude/worktrees/<name>`, `git branch -D <branch>`, `git fetch --prune`.
Before removing one, check it holds no uncommitted or unpushed work and that
its PR is merged; never remove the worktree of a worker that is still
running, and never loop over `.claude/worktrees/*`. Run `cargo clean` in
the main checkout too once no build uses it.
