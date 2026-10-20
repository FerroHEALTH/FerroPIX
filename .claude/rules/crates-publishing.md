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

## One version line

The workspace `version` in the root `Cargo.toml` is the version of every
published crate and the release tag `vX.Y.Z`. Each published crate inherits
it (`version.workspace = true`), so the tag, the crates and the changelog
section always agree. The first published version of each moved crate is
above the one FerroFED published last (`ihe-iti` above 0.0.38,
`nl-generic-functions` above 0.0.24).

## The bump rule

- **A release bumps the workspace version, the changelog section and the
  lock together** (`cargo update -w`), in the release pull request;
  `scripts/checks/crate-version-guard.sh` holds the three together.
- Consumers pin each crate exactly (`ihe-iti = "=0.0.39"`): Cargo reads a
  `0.x` caret requirement as compatible across patches
  (<https://doc.rust-lang.org/cargo/reference/semver.html>), and a product
  moves its pin deliberately.
- **A pull request that breaks a public API says so in its body and its
  CHANGELOG entry**, naming what a product changes; while the line is `0.x`,
  such a release moves the minor version.

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
