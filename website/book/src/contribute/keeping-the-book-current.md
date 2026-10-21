<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Keeping this book current

This book describes only what the code does, so it changes with the code.

- **A change a user can see updates its book page in the same pull
  request**, as it adds its changelog entry: a crate feature, a client's
  behaviour, a specification pin, and, once the server lands, its
  configuration, its deployment files and its endpoints.
- **Work that is not built yet links its issue or milestone** on the
  tracker, and a page never describes it as present.
- **Versions are rendered, never typed.** The crate and product versions on
  these pages and the specification and toolchain tables come from the
  manifests and the pin matrix when the site is assembled, so they cannot go
  stale.
- **Every internal link and anchor resolves.** CI assembles the site the way
  GitHub Pages serves it and checks every link offline on each pull request.

Four checks in CI hold this, each inside the required `conclusion`:

| Check | What it refuses |
|---|---|
| `scripts/checks/book-guard.sh` | A pull request that changes a crate's source or features, the server or a deployment file without touching the book. The `no-book` label lets through a change that alters nothing the book says, such as a refactor. |
| `scripts/checks/docs-claims.sh` | A published crate without its page, a feature without its row or a row without its feature, a typed crate version, and a section that names unbuilt work without linking the tracker. |
| `scripts/checks/site-links.sh` | A broken internal link or anchor anywhere in the assembled site or README, and an mdbook-lint warning. |
| `scripts/checks/writing-style.sh` | An em dash, a word from the banned list, or AI attribution in the published prose. |

What a check can catch is limited: a link check passes on a page whose every
claim is false. The rest is review, which is why the same-pull-request rule
carries the weight.
