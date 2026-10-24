<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Cutting a release

No specification governs the release procedure: our own design. A release
is the signed tag `vX.Y.Z` on the merge commit of the release pull request on
`main`; `.github/workflows/release.yml` does the rest
(`.claude/rules/ci-cd.md`, `.claude/rules/crates-publishing.md`). The
product version is the `[workspace.package]` version and the tag; each
published crate keeps its own version line, moved by the change that alters
it.

## The release pull request

On the branch `release/vX.Y.Z`:

1. **The milestone is empty.** Every issue and pull request of the milestone
   titled `vX.Y.Z` is closed or moved; the release refuses the tag otherwise
   (`scripts/release/milestone.sh check vX.Y.Z`).
2. **Bump the product version.** Set the `[workspace.package]` version in
   `Cargo.toml`, run `cargo update -w`, and set the server image tag
   `compose.yaml` defaults to (`${FERROPIX_VERSION:-X.Y.Z}`) to the same
   version (the console image line takes the same default);
  `scripts/checks/versions.sh` refuses them apart.
3. **Cut the changelog.** Rename `## [Unreleased]` to
   `## [X.Y.Z] - YYYY-MM-DD` and open a fresh, empty `## [Unreleased]` above
   it. The date is the day of the cut: the release notes count the support
   period from it. Read the section as the release notes, and reread the
   reason of a Security section (§ Security entries).
4. **The gates pass.** The crate-version guard checks that the product
   line, the crate lines, the lock and the changelog section agree.

## Security entries

Two rules of Regulation (EU) 2024/2847 hold every Security section, and
`scripts/checks/changelog-security.sh` checks both: CI runs it on every pull
request over `[Unreleased]`, and the release `plan` job over the section it
cuts (`scripts/release/plan.sh`). A section released before the rules is
never checked again.

- **Every entry names its advisory** (Annex I Part II(4), (8)). A Security
  entry carries the `GHSA-` id of the FerroPIX security advisory drafted for
  the fix and published with the release (`SECURITY.md`, Security
  advisories), or, for a dependency finding an OpenVEX statement shows does
  not affect FerroPIX, the path of that statement under `security/vex/`,
  which must exist. A RUSTSEC id alone is not enough: name it beside the
  advisory.
- **A security fix ships alone, or says why not** (Annex I Part II(2)). A
  release whose Security section sits beside any other section opens that
  section with one paragraph, `Shipped with functional changes because:`
  and the reason on the same line, naming why a security-only patch of the
  newest release was not the way to ship it. Whoever adds the first
  Security entry beside other pending changes writes it, and whoever cuts
  the release rereads it.

`scripts/checks/changelog-structure.sh` holds every section to the six Keep a
Changelog types, each at most once.

## Preflight

The `plan` job verifies every release fact at the tagged commit before
anything is built or published. Its checks are one list,
`scripts/release/plan.sh`, and `scripts/release/preflight.sh` runs the same
list on your machine:

```sh
scripts/release/preflight.sh vX.Y.Z
scripts/release/preflight.sh vX.Y.Z --notes /tmp/notes.md   # keep the notes
```

Run it on `release/vX.Y.Z` once the cut is committed, and again on `main`
after the merge, at the commit you are about to tag. It exits 0 when the
`plan` job would pass, names every check that refuses, and prints the path
of the release notes it composed, so you read them before they are
published. You need `gh` logged in, because the milestone check reads the
tracker.

| Check | Script |
|---|---|
| The milestone titled as the tag holds no open item | `scripts/release/milestone.sh check` |
| The `[workspace.package]` version of `Cargo.toml` names the tag's version | `plan.sh` |
| Every section of the changelog holds Keep a Changelog types, each once | `scripts/checks/changelog-structure.sh` |
| The section being cut opens its `### Security` with the reason when it sits beside other changes, and every Security entry names a GHSA id or an existing `security/vex/` path | `scripts/checks/changelog-security.sh --section X.Y.Z` |
| The changelog section becomes the notes, with the support period appended: the month five years after the heading's date (Regulation (EU) 2024/2847 Art 13(19)) | `plan.sh notes` |

`scripts/release/plan.sh prev-tag vX.Y.Z` prints the previous release by
version order from the tags of origin, so a tag pushed whose release never
published still counts. To add a release fact, add a function and a line to
`list()` in `scripts/release/plan.sh`; the `plan` job and the preflight
both pick it up.

## The tag

After the release pull request merges and the preflight passes on `main`,
tag its merge commit signed and push the tag:

```sh
git tag -s vX.Y.Z -m vX.Y.Z <merge-commit>
git push origin vX.Y.Z
```

`release.yml` runs the preflight's list at the tag, takes the notes from
the `[X.Y.Z]` section with the support period appended, builds and attests
the server binaries and the image, records every vulnerability finding,
attests and attaches each listed crate, publishes the release, tags the
image, and its `crates` leg waits on the `crates-io` environment's
reviewer. Its last job closes the milestone. A pushed tag is never moved or
deleted; a bad cut ships forward as a new patch version.

## The build legs

No specification governs the lane's shape: our own design, the family's
(FerroBRIDGE, FerroFED). Every artefact is built in a reusable workflow,
which runs on a VM its caller cannot add steps to, so the signing identity
is out of reach of the build steps: SLSA v1.2 Build Level 3 (SLSA, Build
requirements; GitHub, "Using artifact attestations and reusable workflows
to achieve SLSA v1 Build Level 3").

| Leg | Workflow | What it produces |
|---|---|---|
| `build-binaries` | `release-build.yml`, once per target (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`), each on a runner of its own architecture | `ferropix-vX.Y.Z-<target>.tar.gz` built with `cargo auditable` and no cache, its `.sha256sum`, a CycloneDX source SBOM and an SPDX build SBOM, and the provenance and both SBOMs attested, attached as `.sigstore.json` bundles and an `.intoto.jsonl` |
| `build-image` | `release-image.yml` | verifies both musl tarballs against `release-build.yml` before it unpacks them, stages them with `scripts/release/stage-dist.sh tree`, builds `docker/Dockerfile` for `linux/amd64` and `linux/arm64` with no cache, pushes `ghcr.io/ferrohealth/ferropix` by digest with no tag, attests the index and both manifests (SLSA provenance, an SPDX SBOM per manifest) as OCI referrers, and verifies its own output as a consumer would |
| `build-console` | `release-console.yml` | the operator console: its `binary` job builds the musl binary per architecture with `cargo auditable` and attests the tarball and its SPDX SBOM, its `site` job builds the site bundle with the pinned cargo-leptos, wasm-bindgen CLI and wasm-opt and attests it, both attached to the draft; its `image` job verifies both against `release-console.yml`, builds `docker/console/Dockerfile` for both platforms, pushes `ghcr.io/ferrohealth/ferropix-console` by digest and attests the index and both manifests as `release-image.yml` does |
| `crate` | `release-crate.yml` | each listed `.crate`, its checksum and its attestation |
| `vulnerability-record` | in `release.yml` | Trivy over both pushed images (both platforms each) and the server tarballs at every severity, fixed or not; `scripts/checks/vex-coverage.sh` refuses the release while a finding has no OpenVEX judgement under `security/vex/`, and the record (each finding joined with its statement) and the raw reports are attached to the draft |
| `scan-and-tag` | `scan-and-tag.yml` | the gate of `trivy.yaml` (HIGH and CRITICAL with a fix) on each digest, then the tags `X.Y.Z` and, for a release that is not a pre-release, `X.Y` and `latest`, each verified after it is applied |

`finalize-release` publishes the draft only once every asset of every leg
is attached, and `scan-and-tag` runs only after the vulnerability record,
so a refused release is neither published nor tagged. Verify what a
release published:

```sh
gh attestation verify ferropix-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz \
  --repo FerroHEALTH/FerroPIX \
  --signer-workflow FerroHEALTH/FerroPIX/.github/workflows/release-build.yml
gh attestation verify oci://ghcr.io/ferrohealth/ferropix:X.Y.Z \
  --repo FerroHEALTH/FerroPIX \
  --signer-workflow FerroHEALTH/FerroPIX/.github/workflows/release-image.yml
gh attestation verify oci://ghcr.io/ferrohealth/ferropix-console:X.Y.Z \
  --repo FerroHEALTH/FerroPIX \
  --signer-workflow FerroHEALTH/FerroPIX/.github/workflows/release-console.yml
```

`scripts/release/stage-dist.sh X.Y.Z` stages the published binaries under
`dist/` for a local build of the same image. After the first release
publishes the images, the owner sets the GHCR packages
`ghcr.io/ferrohealth/ferropix` and `ghcr.io/ferrohealth/ferropix-console`
to public visibility and links each to this repository; a new package is
private by default.

## The published image, every week

`image-scan.yml` scans both images at `latest` each Monday
with `trivy.yaml` and every OpenVEX document under `security/vex/`
applied, and files or comments on one tracking issue when a finding
remains. An image not yet published is named and skipped.
`scripts/security/scan-images.sh` runs the same scan locally, and with
`--candidate <ref>` over an image you built.

## A security fix

A reported vulnerability gets a private draft GitHub security advisory the
day it is confirmed (`SECURITY.md`, Security advisories). The fix ships in a
patch release of the newest version, alone where that is technically
feasible. Its Security entry names the draft's `GHSA-` id; a dependency
finding that does not affect FerroPIX names its OpenVEX statement under
`security/vex/` instead, and gets no advisory. When the release carries
other changes too, the Security section opens with its reason (§ Security
entries). Publish the advisory, with the release version as its patched
version, in the same hour as the release.

An accepted advisory in `deny.toml` needs its statement in
`security/vex/rust-advisories.toml`, and `scripts/security/vex-generate.sh`
regenerates the published document (`security/vex/README.md`). A finding in
the server image or its binary that does not affect FerroPIX gets its
statement in an OpenVEX document under `security/vex/` with the product
`pkg:oci/ferropix` or `pkg:cargo/ferropix-server`, which the release's
vulnerability record and the weekly image scan both read.
