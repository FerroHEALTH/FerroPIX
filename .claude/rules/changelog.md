---
paths:
  - "CHANGELOG.md"
  - "changelog.d/**"
  - "scripts/release/**"
  - "scripts/checks/changelog-*.sh"
---

<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# The changelog: fragments, assembled at the cut

`CHANGELOG.md` follows Keep a Changelog 1.1.0
(<https://keepachangelog.com/en/1.1.0/>), and the release notes are its
section for the tag. No specification governs how entries reach it: our own
design, the family's (FerroEHR, FerroFED, FerroTERM, FerroUI).

## The entry rule

- **A change with user-visible effect adds one fragment,**
  `changelog.d/<issue>-<kebab-slug>.<section>.md`, in the same pull request.
  `<section>` is `added`, `changed`, `deprecated`, `removed`, `fixed` or
  `security`. The name, the content and the issue reference the cut adds are
  in `changelog.d/README.md`.
- **No pull request writes under `[Unreleased]` in `CHANGELOG.md`.** That
  section is what two open pull requests used to conflict on.
  `scripts/checks/changelog-guard.sh` refuses it, also under the
  `no-changelog` label.
- **User-visible** means what a caller, an operator or a crate user sees: the
  FHIR and HL7 v2 faces, the operator API and the console, matching and
  storage, configuration, the CLI, the images, compose and the Helm chart,
  and a published crate's behaviour or features. The guard asks for a
  fragment when a change touches `app/`, `crates/`, `deploy/`, `docker/`,
  `compose.yaml` or the Cargo manifests; the `no-changelog` label waives it
  for a change none of those readers sees.
- **A breaking change says so** in its fragment and names what a caller
  changes (`.claude/rules/crates-publishing.md` for a published crate).

## Security entries

A `security` fragment names its advisory: a `GHSA-` id or an existing path
under `security/vex/`. A release whose Security section sits beside other
sections opens it with the reason paragraph, a `security-reason` fragment.
`docs/release.md` § Security entries gives both rules and their ground in
Regulation (EU) 2024/2847; `scripts/release/changelog.sh --check-preview`
checks them on every pull request, over `[Unreleased]` with the fragments
folded in.

## The cut

The release pull request runs
`scripts/release/changelog.sh --assemble X.Y.Z YYYY-MM-DD`, which writes the
version's section from the fragments, as a tight list, and deletes them
(`docs/release.md` § The release pull request). The release `plan` job
refuses a tag while a fragment is left under `changelog.d/`.
