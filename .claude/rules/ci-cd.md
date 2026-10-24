---
paths:
  - ".github/**"
  - "scripts/**"
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# CI/CD and supply-chain discipline

No specification governs this: our own design, grounded in the OWASP GitHub
Actions Security Cheat Sheet, SLSA v1.0, OpenSSF Scorecard, and Sigstore.

## What runs

- `.github/workflows/ci.yml`: the gate on every pull request, merge-group run
  and push to `main`. The workflow tier (zizmor, actionlint, shellcheck, the
  self-test of every helper and guard, the comment-style guard, the pin and
  licence guard `versions.sh`, the file-length guard, the `TODO(#N)` guard,
  the released-changelog guard, and on pull requests the changelog guard,
  the contribution-licence guard and the crate-version guard, and hadolint
over every Dockerfile) and the Rust
  tier once the workspace has a member (`cargo fmt --all --check`; `cargo
  clippy --workspace --all-targets --locked -- -D warnings` with and without
  `--all-features`; `cargo nextest run --workspace --locked` with and
  without `--all-features`, plus `cargo test --doc`; `cargo doc` with
  `RUSTDOCFLAGS=-D warnings`; `cargo deny check`; the publish dry run of
  every listed crate; `dependency-review-action` on pull requests; the
`quickstart` job, which builds the server for the musl target, builds the
image from the staged tree and runs `docker compose up --wait` to
readiness with the usage report off, and builds the console image from a
placeholder tree). The
  `manifest` job reads the root manifest under the pinned toolchain even
  while the workspace is empty. The `conclusion` job is the single required
  status check on `main`.
- `.github/workflows/codeql.yml`: CodeQL over the `actions` language and Rust.
- `.github/workflows/scorecard.yml`: OpenSSF Scorecard, published to the
  OpenSSF API and to code scanning.
- `.github/workflows/sonar.yml`: SonarQube Cloud over the Rust workspace,
  with the lcov report of an instrumented run, and over the shell, YAML and
  JSON, on every pull request and push to `main`. Advisory: `conclusion`
  does not need it, and it skips without the `SONAR_TOKEN` secret
  (`ai-code-review.md`).
- `.github/workflows/pin-freshness.yml`: each Monday and on dispatch,
  `scripts/checks/pin-freshness.sh` reads every pin no Dependabot ecosystem
  watches (the CI tool versions in `ci.yml`, the documentation toolchain,
  the corpus pins and the container images of `docs/VERSIONS.md`) against
  its newest upstream release and opens one issue through
  `scripts/gh/fields.sh new` when one is behind. A pull request that changes
  the lane, `ci.yml` or the pin matrix runs it as a dry run that files
  nothing. A new corpus row needs a reader there, or its vendor script in
  `UNWATCHED_SCRIPTS` with the reason, or the self-test fails.
- `.github/workflows/fuzz.yml`: each Thursday and on dispatch for five
  minutes per target, and for one minute on a pull request that touches
  what a target reads (`ihe-iti`, the root manifest, `fuzz/`, the vendored
  PIXm and PDQm examples), cargo-fuzz runs the targets of `fuzz/` on the
  nightly `docs/VERSIONS.md` pins and uploads the input of a crash or a
  timeout as an artifact. Its `inputs` job holds the generated seeds to the
  vendored examples, the trigger paths to the fuzz crate's path
  dependencies and the matrix to its targets. Advisory: `conclusion` does
  not need it. CI's `fuzz-lockfile` job, which `conclusion` does need,
  holds `fuzz/Cargo.lock` in step with the crates it builds.
- `.github/workflows/release.yml`: dormant until a `v*` tag is pushed. It
  validates the tag against the workspace version, takes the notes from the
  matching `CHANGELOG.md` section, creates the release as a draft, and calls
  four reusable workflows, so the signing identity stays out of reach of
  caller steps (SLSA v1.2 Build Level 3): `release-build.yml` builds the
  musl server binary per architecture with `cargo auditable` and attests the
  tarball and its two SBOMs; `release-image.yml` verifies those tarballs
  against `release-build.yml`, pushes `ghcr.io/ferrohealth/ferropix` for
  both platforms by digest and attests the index and both manifests as OCI
  referrers; `release-console.yml` builds and attests the console's musl
  binary and site bundle, verifies both and pushes and attests
  `ghcr.io/ferrohealth/ferropix-console` the same way; `release-crate.yml`
  packages, attests and attaches each `.crate`. Its `vulnerability-record` job scans the image and the tarballs
  at every severity and refuses the release while a finding lacks an
  OpenVEX judgement (`scripts/checks/vex-coverage.sh`), and `scan-and-tag.yml`
  gates the digest on fixable HIGH and CRITICAL findings before it applies
  the tags. It publishes the release once every asset is attached, and its
  `crates` leg uploads the crates through Trusted Publishing behind the
  `crates-io` environment (`crates-publishing.md`). `docs/release.md` is the
  lane.
- `.github/workflows/image-scan.yml`: each Monday, Trivy over both published
  images at `latest` with every OpenVEX document applied; a finding files or
  comments on one tracking issue, and an image not yet published is skipped.
  Its `plan` job refuses a tag when no milestone is titled exactly as the tag
  or that milestone still holds an open issue or pull request, naming each
  one, and its last job, `milestone`, closes that milestone once every leg,
  crates included, has succeeded; a failed leg leaves it open and the job log
  names the leg. Both call `scripts/release/milestone.sh`.
- `.github/workflows/publish-crates.yml`: the manual crates.io lane, a dry
  run unless `publish` is set, sharing `scripts/release/publish-crates.sh`
  with the release lane.

## Workflow security (every workflow, no exceptions)

- **Every `uses:` is pinned to a full commit SHA** with a trailing `# vX.Y.Z`
  comment; Dependabot (`github-actions`) bumps them.
- **`permissions: {}` at workflow level**, with the minimum granted per job.
- **`persist-credentials: false`** on every `actions/checkout`.
- **No `${{ }}` context interpolation inside `run:`:** pass context through
  `env:`.
- **A publishing lane restores no build cache.**

Run `zizmor --min-severity=low .github/`, `actionlint` and `shellcheck
--severity=style` by hand before pushing a workflow or script change. A
finding is fixed, or carries a `# zizmor: ignore[audit]` or `# shellcheck
disable=SCnnnn` with its reason on the line it names; no `.shellcheckrc`
exists.

## Supply chain

- **A release is assembled as a draft and published last.** The fix for a bad
  cut is a new patch version, never a retag: immutable releases freeze a
  published release, and the `release-tags` ruleset stops a tag being moved
  or deleted.
- **Every tag is signed**, on the merge commit on `main`.
- **Always `--locked`**, and `Cargo.lock` is committed.
- **The crates publish through Trusted Publishing** (OIDC, no long-lived
  token), with a dry run on every pull request.

## Never

- Never unpin a `uses:`, widen a job's permissions without cause, interpolate
  context into `run:`, or restore a cache in a publishing lane.
- Never weaken a gate to go green (`testing.md`); fix the cause.
- **Never add AI or Claude attribution** to any commit, PR, or release text.

## Official documentation (durable citations)

- OWASP GitHub Actions Security Cheat Sheet:
  <https://cheatsheetseries.owasp.org/cheatsheets/GitHub_Actions_Security_Cheat_Sheet.html>
- SLSA v1.0: <https://slsa.dev/spec/v1.0/>
- OpenSSF Scorecard: <https://github.com/ossf/scorecard-action>
- Trusted Publishing on crates.io: <https://crates.io/docs/trusted-publishing>
