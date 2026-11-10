<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Keeping this book current

This book describes only what the code does, so it changes with the code.

- **A change a user can see updates its book page in the same pull
  request**, as it adds its changelog entry: the server's endpoints, its
  configuration, its deployment files, the console, a crate feature, a
  client's behaviour, a specification pin.
- **Work that is not built yet links its issue or milestone** on the
  tracker, and a page never describes it as present.
- **Versions are rendered, never typed.** The crate and product versions on
  these pages and the specification and toolchain tables come from the
  manifests and the pin matrix when the site is assembled, so they cannot go
  stale.
- **Every internal link and anchor resolves.** CI assembles the site the way
  GitHub Pages serves it and checks every link offline on each pull request.
- **Every sample runs.** A quickstart command is a step of a script CI runs,
  and a configuration block is read by the loader of the program it
  configures.

These checks in CI hold this, each inside the required `conclusion`:

| Check | What it refuses |
|---|---|
| `scripts/checks/book-guard.sh` | A pull request that changes a crate's source or features, the server or a deployment file without touching the book. The `no-book` label lets through a change that alters nothing the book says, such as a refactor. |
| `scripts/checks/docs-claims.sh` | A published crate without its page, a feature without its row or a row without its feature, a typed crate version, and a section that names unbuilt work without linking the tracker. |
| `scripts/checks/site-links.sh` | A broken internal link or anchor anywhere in the assembled site or README, and an mdbook-lint warning. |
| `scripts/checks/writing-style.sh` | An em dash, a word from the banned list, or AI attribution in the published prose. |
| `scripts/quickstart/tour.sh`, in the `quickstart` job | A quickstart command that fails, or answers other than the page says, against the images the change builds. The page includes each command from the script by its mdBook anchor, and `--self-test` refuses an anchor the page names and the script lacks, and a copy in `README.md` that differs from its anchor by a byte. |
| `tests/it/book.rs` of the server and of the console | A key of the configuration tree missing from the configuration page, and a `toml` block the server's loader refuses (a `toml,console` block, the console's loader). A block that configures another product says so in its info string, such as `toml,ferroterm`, and is not read. |

What a check can catch is limited: a link check passes on a page whose every
claim is false. The rest is review, which is why the same-pull-request rule
carries the weight.
