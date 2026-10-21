---
paths:
  - "website/**"
  - "assets/brand/**"
  - "scripts/site/**"
  - "scripts/checks/docs-claims.sh"
  - "scripts/checks/book-guard.sh"
  - "scripts/checks/site-links.sh"
  - "scripts/checks/writing-style.sh"
  - ".github/workflows/docs.yml"
  - ".github/actions/docs-toolchain/**"
  - ".mdbook-lint.toml"
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The documentation site (`website/**`)

The public site, <https://ferropix.eu/>, is built from `website/` by
`scripts/site/assemble.sh` and deployed to GitHub Pages by
`.github/workflows/docs.yml`. No specification governs the site: our own
design, after the FerroHEALTH siblings (FerroFED, FerroTERM, FerroEHR,
FerroBRIDGE) and the family book. This rule is the authority for its layout,
its look and its content. The same-PR rule below is also a hard rule in
`CLAUDE.md`, because a change outside `website/` is what triggers it.

## Layout and URLs

- `website/landing/`: the hand-written landing page at the site root.
  Relative URLs only, light and dark through `prefers-color-scheme`, a
  `<meta>` Content-Security-Policy with every source `'self'`, and no request
  to another origin. The roadmap block between `<!-- roadmap:begin -->` and
  `<!-- roadmap:end -->` is rendered from the open milestones (name, theme,
  link; never issue counts, which go stale between deploys).
- `website/book/`: the mdBook, served at `/docs/` (`site-url`). mdBook's
  `light` and `navy` themes recoloured in `theme/ferropix.css` with the "Plum
  & Iron" palette of `assets/brand/tokens.css`; system fonts
  (`theme/fonts/fonts.css`); the mark beside the title from
  `assets/brand/` at the site root.
- The book is built only through `scripts/site/assemble.sh`, from a staged
  copy: it adds the favicon from `assets/brand/` (so the artwork exists once),
  renders the version markers, and gives every page a hashed CSP
  (`scripts/site/book-csp.sh`). A plain `mdbook build` shows raw markers.
- The custom domain is the repository's Pages setting, which
  `actions/deploy-pages` keeps; the site carries no `CNAME` file.
- The book is not versioned: it documents `main`, as FerroFED's and
  FerroBRIDGE's do. Per-release frozen books, as FerroEHR has, wait until
  there are server releases to diverge.

## Family documentation lives in the FerroHEALTH book

Documentation about the family or the manufacturer is written once, in the
family book at <https://ferrohealth.eu/docs/> (FerroHEALTH/FerroHEALTH), and
this book links it: the manufacturer (`manufacturer.html`), security
(`security.html`), post-market (`post-market.html`), the CRA (`cra.html`),
licensing and trademarks (`licensing.html`), and how the products fit
together (`architecture.html`). Before writing a page here, ask whether it
is about FerroPIX alone; if it is about the family or the manufacturer, it
goes to the family book.

## The same-PR rule

**A pull request that changes what a user of FerroPIX sees updates the
matching page under `website/book/src` in the same pull request**, as it adds
its changelog entry: a published crate's behaviour or features, and, once
they exist, the server's endpoints, configuration and deployment files.
`scripts/checks/book-guard.sh` (the `book-guard` job of CI, inside
`conclusion`) refuses a change to `crates/*/src/**`, a crate's
`[features]`, `app/**`, `docker/**`, `deploy/**` or a compose file that does
not touch the book; the `no-book` label is the escape for a change that
alters nothing the book says (a refactor, a fix that keeps the documented
behaviour). `/phase-done` checks it at close.

## Authoring rules

- The writing style is `.claude/rules/writing-style.md`; the mechanical half
  is `scripts/checks/writing-style.sh`, run in CI over the published prose.
- Second person, task-first. The audiences are evaluators, integrators,
  operators and contributors, never repo-internal readers.
- **Say what is built and what is planned.** A capability that is not built
  is named as planned and links its issue or milestone on a FerroHEALTH
  tracker; a page never describes it as present. This includes a sibling's
  wiring to FerroPIX: say it as planned until that sibling's change merges.
- **Never type a version that moves.** Write `{{version:<crate>}}`,
  `{{version:ferropix}}` or `{{dep:<name>}}`; the specification and
  toolchain tables are `{{#include}}`d from the anchors of
  `docs/VERSIONS.md`, and the changelog from `CHANGELOG.md`.
- Every feature name, type, method and behaviour is checked against the code
  (the crate's `Cargo.toml`, its rustdoc, its README) before it is written.
- Never publish or link `.claude/**`, `CLAUDE.md` or the vendored
  `docs/specs/**`; paraphrase into user language and link the published
  specification instead.
- Callouts are mdBook 0.5's `> [!NOTE]`, `[!WARNING]` and `[!TIP]`.

## What a gate checks, and what only review can

**Machine-checked**, each in a job the required `conclusion` needs:

- `site-links.sh`: mdbook-lint over the sources (`.mdbook-lint.toml`, a
  warning fails), the site assembled as Pages serves it, every internal link
  and anchor resolved offline, and README.md against the tree.
- `docs-claims.sh`: every published crate has its page and its row on the
  crates page, its Features table names exactly the features of its
  `Cargo.toml`, no page types its current version, and every section that
  says "planned" links a tracker issue or milestone.
- `book-guard.sh`: the same-PR rule above.
- `writing-style.sh`: em dashes, the buzzword list and AI attribution.

**Review-only:** prose claims of behaviour ("the client refuses an answer
without its score") have no machine authority. That is why the same-PR rule
carries the weight. When a guard reports something, check whether the guard
is wrong before editing the page.

## Toolchain

mdBook, mdbook-lint and lychee, pinned in the "Documentation toolchain"
table of `docs/VERSIONS.md`, read by `scripts/site/toolchain.sh` and
installed by `.github/actions/docs-toolchain`. They are the family book's
pins; move one in step with the family, after building the book with it.

## Local preview

```sh
scripts/site/assemble.sh _site
python3 -m http.server --directory _site 8000
scripts/checks/site-links.sh
```

Before a website pull request merges, look at the landing page and a book
page at 1440 and 390 pixels, light and dark: the page itself never scrolls
sideways, and a wide code block or table scrolls inside its own box.
