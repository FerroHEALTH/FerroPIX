<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.2] - 2026-10-20

The move from FerroFED (FerroHEALTH/FerroFED#367): `ihe-iti` 0.0.39 and
`nl-generic-functions` 0.0.25 are published from FerroPIX, each copied with
its history and with the corpora it reads re-vendored at FerroFED's pins.
FerroFED pins both by exact version from crates.io.

### Added

- `ihe-iti` 0.0.39, moved from FerroFED with its history: the IHE PIXm,
  PDQm, PMIR, mCSD and XCPD clients and the IUA authorizer, a feature each.
  The `atna` and `balp` features are gone: the audit trail is the `ihe-atna`
  crate of FerroAUDIT. The audit record each profile defines for its own
  transactions stays here behind the new `audit` feature, on `ihe-atna`
  `=0.1.0`. A product changes `ihe_iti::user` to `ihe_atna::user`,
  `ihe_iti::balp` and `ihe_iti::atna` to `ihe_atna::balp` and
  `ihe_atna::atna`, and the features `balp` and `atna` to `audit` (#25).
- `nl-generic-functions` 0.0.25, moved from FerroFED with its history: NVI
  localization, the Mitz closed authorization question, LRZa addressing and
  the Nuts access token request, a feature each. `nuts-auth` no longer
  depends on `oauth-server-metadata`, which is retired: it takes the RFC
  8414 issuer rule and metadata location from `ferrohealth-smart` `=0.3.1`.
  A plain `http` issuer is now accepted on a loopback host only (#26).
- The IHE citation guard, `scripts/checks/ihe-citations.sh`, and the corpora
  its audit citations read at FerroFED's pins: the BALP package and
  narrative pages and the ITI-20 page with the RESTful ATNA supplement
  (#25).

- The corpora `ihe-iti` and `nl-generic-functions` read, vendored under
  `docs/specs/` by FerroFED's scripts at FerroFED's pins and byte-identical
  to FerroFED's copies: the IHE PIXm, PDQm, PMIR and mCSD packages and
  narrative pages, the ITI Technical Framework pages, the IUA supplement,
  the Swiss pins of PIXm, PDQm and IUA, the Netherlands Generic Functions IG
  source, the Mitz documents (cache only), the Nuts RFCs, the IETF OAuth
  RFCs, the W3C DID and VC texts and DIF Presentation Exchange, each with
  its `scripts/vendor/*.sh`, its `PROVENANCE.md` and its row in
  `docs/VERSIONS.md` (#24).

### Changed

- The minimum supported Rust version is 1.98, the version `ihe-atna`
  requires (#25).
- Two version lines, the model FerroBRIDGE uses: the product version is the
  workspace version and the release tag, and each published crate keeps its
  own version, which never adopts the product version. The crate-version
  guard holds both, and refuses a change to a crate's packaged content that
  does not bump it unless the pull request carries `no-crate-bump`. The
  release lane names each crate's files by the crate's own version (#23).

## [0.0.1] - 2026-10-20

The repository setup: the working discipline, the tracker helpers and the
guards, CI and the release lane, the workspace skeleton and the GitHub
settings. No crate is published in this release; the first crates arrive
with the move from FerroFED in 0.0.2.

### Added

- The tracker helpers under `scripts/gh/` (`fields.sh`, `rel.sh`,
  `labels.sh`, `watch-prs.sh`, `changelog-merge.sh`, and `project.sh`, which
  says FerroPIX has no roadmap board) and the guards under `scripts/checks/`
  (comment style, the pin matrix and licence headers, file length, `TODO`
  pointers, the crate version, the changelog and the contribution licence),
  each with a `--self-test` (#11).
- CI with the single required `conclusion` check, CodeQL, OpenSSF Scorecard,
  Dependabot, the release lane that refuses a tag whose milestone is not
  empty and closes it after the release, the crates.io lane through Trusted
  Publishing, and the pull request and issue templates (#12).
- `docs/VERSIONS.md`, the pin matrix the guards read (#11).
- The working discipline: `CLAUDE.md` with the scope the owner settled on
  2026-10-20, the rules, the `spec-researcher` and `implementer` agents, the
  `spec-lookup`, `next-task`, `phase-done` and `phase-status` skills, the
  hooks (the destructive-command hook with its self-test) and the memory
  (#13).
- The specification versions of every identity profile that moves from
  FerroFED in `docs/VERSIONS.md` (#13).
- `SUPPORT.md`, `GOVERNANCE.md` and `CODE_OF_CONDUCT.md`, and a README and
  CONTRIBUTING.md that name what FerroPIX is, how to build and test it and
  the specifications that govern it (#14).
- The owner's rule for moving the vendored corpora with the crates, in
  `.claude/rules/vendored-inputs.md` and the memory (#20).
- The Cargo workspace skeleton: the root manifest with the family lint set
  and the release profile, the pinned toolchain, and the cargo-deny, clippy
  and rustfmt configuration. The workspace has no member until the move from
  FerroFED; CI reads the manifest under the pinned toolchain meanwhile (#15).

### Changed

- `SECURITY.md` supports only the newest release, with the Cyber Resilience
  Act commitment to publish security fixes for at least five years after
  each release (#14).
