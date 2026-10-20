<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

### Changed

- `SECURITY.md` supports only the newest release, with the Cyber Resilience
  Act commitment to publish security fixes for at least five years after
  each release (#14).
