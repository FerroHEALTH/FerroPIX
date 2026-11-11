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
  line, judged by the Cargo SemVer rules below. Both crates came from
  FerroFED on its 0.0.z line and are on 0.2.z since 2026-11-10.

## SemVer: the Cargo rules, judged by cargo-semver-checks

Every FerroHEALTH repository that publishes a crate follows Cargo's SemVer
rules, checked by `cargo-semver-checks`, with no rules of its own (owner,
2026-11-10, #586). The authority is The Cargo Book, "SemVer Compatibility"
(<https://doc.rust-lang.org/cargo/reference/semver.html>): in a 0.y.z
version "only changes in the left-most non-zero component are considered
incompatible", so while a crate is 0.x a breaking change moves the minor
(0.2.4 to 0.3.0) and a compatible change moves the patch.

- **The judge is `scripts/checks/semver.sh`**, over `cargo-semver-checks` at
  the version `docs/VERSIONS.md` pins
  (<https://github.com/obi1kenobi/cargo-semver-checks>). It runs twice per
  crate: on the tool's default feature selection, and with
  `--only-explicit-features`, the build a consumer with
  `default-features = false` compiles against, since the tool does not
  catch a break that exists only under a subset of the features (its
  README). `ihe-iti` keeps one feature per profile, so that subset is a real
  consumer's build. Both crates are judged; neither is skipped.
- **A public dependency on another compatibility line is a break.** A type
  of the dependency that the crate exposes becomes another type, so a
  consumer that built before fails to build, the breaking change the
  chapter's introduction defines; `cargo-semver-checks` lists breaking type
  changes among what it does not catch. The public dependencies are
  `ihe-atna`, `ferrohealth-*`, `fhir-types`, `fhir-search`, `hl7v2-types`,
  `ferrobridge-*` and the sibling crate (`scripts/checks/lib/public-deps.sh`),
  each read by the line Cargo's caret rule gives its requirement.
- **A version step is one SemVer step**: the patch, or a higher minor or
  major with the components after it at 0 (SemVer 2.0.0 §7 and §8,
  <https://semver.org/>). A minor with no break is allowed; the chapter
  takes no stance on it.
- **Where it runs.** `scripts/checks/crate-version-guard.sh` holds the step
  and the dependency lines and names each crate whose version moved; the
  `crate-version-guard` job then runs the judge over them against the
  target branch, on every pull request and in the merge queue. The release
  plan (`scripts/release/plan.sh`, `publish-crates.sh semver`) judges every
  crate the release publishes against its newest release on crates.io before
  the `crates` leg uploads.

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
  request, in the merge queue and in `scripts/gh/watch-prs.sh`.
- Consumers pin each crate exactly (`ihe-iti = "=0.2.0"`) and move the pin
  deliberately; a caret requirement (`ihe-iti = "0.2.0"`) takes every
  compatible patch of the 0.2 line, which Cargo's caret rule reads as
  compatible
  (<https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#caret-requirements>).
- **A shared crate takes a caret requirement on another family shared
  crate** (owner decision 2026-10-26): `ihe-iti` requires
  `ihe-atna = "0.13.0"` and `ferrohealth-smart = "0.19.0"`, and
  `nl-generic-functions` requires `ferrohealth-smart = "0.19.0"`, never an
  `=` pin, because the FerroAUTH, FerroAUDIT and FerroSYS lines never break
  on a patch. A patch of theirs then reaches a product through its lockfile
  without a release of these crates first. A product, the FerroPIX server
  included, still pins every FerroAUTH, FerroAUDIT, FerroSYS and FerroUI
  crate exactly in `[workspace.dependencies]`.
- **Every FerroBRIDGE crate is required by a caret range** (`fhir-types`,
  `fhir-search`, `hl7v2-types`, `ferrobridge-*` and the rest), with its floor
  at the release this tree needs, in `[workspace.dependencies]` and so in
  the published crates that take it from there. FerroBRIDGE's crates follow
  Cargo SemVer, judged by cargo-semver-checks in its release plan
  (FerroHEALTH/FerroBRIDGE#1745), so a caret range is safe, and two family
  crates on different patches of one FerroBRIDGE crate build together (#543).
- **A pull request that breaks a public API says so in its body and its
  CHANGELOG entry**, naming what a product changes.

## The publish lane is resumable and verified

`scripts/release/publish-crates.sh` (`publish` / `dry-run` / `package` /
`verify` / `version` / `list`) uploads the crates its `CRATES` array lists, in dependency order,
counts "already exists" as done, and reads the registry back before
reporting success. Two lanes call it: the `crates` leg of `release.yml` on a
`v*` tag (the primary path, paused by the `crates-io` environment's required
reviewer) and `publish-crates.yml` on a manual dispatch (a dry run by
default; `publish = true` is the recovery path). Both authenticate with
crates.io Trusted Publishing (OIDC through `rust-lang/crates-io-auth-action`);
no long-lived crates.io token exists in the repository. Neither lane restores
a build cache (`ci-cd.md`). While `CRATES` is empty, the release lane
publishes the notes alone and skips both crate legs.

`publish-crates.sh dry-run` and `publish-crates.sh package DIR` run on every
pull request in the `publish-dry-run` job. Both take every listed crate in
one cargo call, so a crate that requires a sibling not yet on crates.io
resolves it from the set; `release-crate.yml` packages the `.crate` files it
attests through the same `package` (#446).

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

- The Cargo Book, SemVer Compatibility: <https://doc.rust-lang.org/cargo/reference/semver.html>
- cargo-semver-checks: <https://github.com/obi1kenobi/cargo-semver-checks>
- Semantic Versioning 2.0.0: <https://semver.org/>
- Trusted Publishing on crates.io: <https://crates.io/docs/trusted-publishing>
- `cargo publish`: <https://doc.rust-lang.org/cargo/commands/cargo-publish.html>
