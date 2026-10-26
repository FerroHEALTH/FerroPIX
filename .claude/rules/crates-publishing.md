<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Published crate discipline (crates.io)

FerroPIX publishes the crates of the identity specifications it owns, so the
FerroHEALTH products depend on them by exact version: `ihe-iti` (the IHE
identity profiles: PIXm, PDQm, PMIR, mCSD, XCPD and IUA, a feature each) and
`nl-generic-functions` (the Dutch Generic Functions), both arriving with the
move from FerroFED (v0.0.2). A published crate carries the name of the
specification it implements, never `ferropix-*`; FerroPIX's own server glue
lives under `app/` with a hard `publish = false`. The crates are BUSL-1.1.
Published versions are immutable, so version hygiene is a hard rule.

## Two version lines

FerroPIX takes the model FerroBRIDGE uses (owner, 2026-10-20,
FerroHEALTH/FerroFED#367; no specification governs it: our own design):

- **The product version** is the workspace `version` in the root
  `Cargo.toml`: the server glue under `app/`, the release tag `vX.Y.Z` and
  the `CHANGELOG.md` section. `release.yml` refuses a tag that differs from
  it.
- **The crate line** is the `version` in each published crate's own
  `crates/<name>/Cargo.toml`. It never adopts the product version
  (`version.workspace = true` fails the guard); it is the crate's own SemVer
  line. Each moved crate continues the line it had in FerroFED: `ihe-iti`
  from 0.0.39 (FerroFED's last was 0.0.38), `nl-generic-functions` from
  0.0.25 (FerroFED's last was 0.0.24), though crates.io holds only their
  0.0.0 reservations.

## The bump rule

- **A pull request that changes the packaged content of a published crate
  moves that crate's own version in the same pull request.** Packaged
  content is what the crate's `include` ships (`src/**`, `README.md`,
  `LICENSE`, `Cargo.toml`), plus a root `[workspace.dependencies]` entry the
  crate consumes, because `cargo package` renders the concrete requirement.
  Tests and `CLAUDE.md` are not packaged and need no bump.
- The crate's manifest and `Cargo.lock` move together (`cargo update -w`).
  A crate's version never moves backwards.
- **A release moves the product version and adds its changelog section** in
  the release pull request; it moves no crate version by itself.
- Escape: the `no-crate-bump` pull-request label, only when the diff
  provably alters no packaged byte.
- Not every bumped version is published; gaps in the published sequence are
  normal. Publishing different content under an existing version is what is
  forbidden, and crates.io refuses it.
- `scripts/checks/crate-version-guard.sh` holds all of this, on every pull
  request and in `scripts/gh/watch-prs.sh`.
- Consumers pin each crate exactly (`ihe-iti = "=0.0.39"`): Cargo reads a
  `0.x` caret requirement as compatible across patches
  (<https://doc.rust-lang.org/cargo/reference/semver.html>), and a product
  moves its pin deliberately.
- **A shared crate takes a caret requirement on another family shared
  crate** (owner decision 2026-10-26): `ihe-iti` requires
  `ihe-atna = "0.2.13"` and `ferrohealth-smart = "0.7.4"`, and
  `nl-generic-functions` requires `ferrohealth-smart = "0.7.4"`, never an
  `=` pin, because the FerroAUTH, FerroAUDIT and FerroSYS lines never break
  on a patch. A patch of theirs then reaches a product through its lockfile
  without a release of these crates first. A product, the FerroPIX server
  included, still pins every shared crate exactly in
  `[workspace.dependencies]`.
- **A pull request that breaks a public API says so in its body and its
  CHANGELOG entry**, naming what a product changes.

## The publish lane is resumable and verified

`scripts/release/publish-crates.sh` (`publish` / `verify` / `version` /
`list`) uploads the crates its `CRATES` array lists, in dependency order,
counts "already exists" as done, and reads the registry back before
reporting success. Two lanes call it: the `crates` leg of `release.yml` on a
`v*` tag (the primary path, paused by the `crates-io` environment's required
reviewer) and `publish-crates.yml` on a manual dispatch (a dry run by
default; `publish = true` is the recovery path). Both authenticate with
crates.io Trusted Publishing (OIDC through `rust-lang/crates-io-auth-action`);
no long-lived crates.io token exists in the repository. Neither lane restores
a build cache (`ci-cd.md`). While `CRATES` is empty, the release lane
publishes the notes alone and skips both crate legs.

`cargo publish -p <crate> --dry-run --locked` runs for every listed crate on
every pull request as the `publish-dry-run` job.

## Owner steps (once per crate)

1. The name is held on crates.io by a 0.0.0 placeholder the owner publishes,
   since a first version cannot use Trusted Publishing. `ihe-iti` and
   `nl-generic-functions` have theirs.
2. On crates.io, the crate's Trusted Publishing carries two GitHub entries:
   repository owner `FerroHEALTH`, repository `FerroPIX`, workflows
   `release.yml` and `publish-crates.yml`, environment `crates-io`.
3. The `crates-io` GitHub environment carries the owner as required reviewer
   and admits `main` and `v*` tags (set on 2026-10-20).
4. The crate is added to `CRATES` in `scripts/release/publish-crates.sh` in
   the change that makes it publishable.

## Official documentation (durable citations)

- Trusted Publishing on crates.io: <https://crates.io/docs/trusted-publishing>
- `cargo publish`: <https://doc.rust-lang.org/cargo/commands/cargo-publish.html>
