<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# CLAUDE.md

**FerroPIX** is the FerroHEALTH family's Master Patient Index over IHE PIX,
PIXm, PDQ and PDQm, in pure Rust. It holds the patient identity
cross-reference and mints the pseudonymous identifiers the clinical domain
uses, so identifying data stays outside the clinical record. FerroEHR
resolves `ehr_id` through it (PIXm `$ihe-pix` ITI-83 and PDQm ITI-78), and
FerroFED's identity binding points at it.

FerroPIX in prose, `ferropix` in identifiers.

## Scope (owner, 2026-10-20)

- FerroPIX owns and publishes two crates named for the specifications they
  implement: `ihe-iti` (the IHE identity profiles PIXm, PDQm, PMIR, mCSD,
  XCPD and IUA, a feature each, with the server sides added as features) and
  `nl-generic-functions` (the Dutch Generic Functions: BSN and the BSNk
  pseudonyms, NVI localization, Mitz and Nuts). FerroFED pins both by exact
  version. FerroPIX's own server glue lives under `app/` as `ferropix-*` with
  `publish = false`.
- Two version lines, as FerroBRIDGE has: the product version is the
  workspace version and the release tag; each published crate keeps its own
  version line, bumped by the change that alters it
  (`.claude/rules/crates-publishing.md`).
- It serves FHIR and HL7 v2 together, on FerroBRIDGE's v2 parsing
  (`hl7v2-types` and its MLLP codec and parser), never a second parser.
- The ATNA and BALP audit senders live in FerroHEALTH/FerroAUDIT as the crate
  `ihe-atna`; FerroPIX audits the Manager side through it.
  `oauth-server-metadata` is retired; authorization comes from
  `ferrohealth-smart` (FerroSMART).

The build order is the milestones:

- **v0.0.1, the repository setup:** this working discipline, the tracker
  helpers and guards, CI and the release lane, the workspace skeleton, the
  GitHub settings. No code moves.
- **v0.0.2, the move from FerroFED** (FerroHEALTH/FerroFED#367): `ihe-iti`
  without the `atna` and `balp` features, and `nl-generic-functions`, each
  copied with its history and its corpora re-vendored by FerroFED's scripts
  at FerroFED's pins, published from here above FerroFED's versions
  (`ihe-iti` 0.0.39 or later, `nl-generic-functions` 0.0.25 or later).
- **v0.0.3, the server:** the PIXm Manager (ITI-83 by GET and POST with
  several `targetSystem`, ITI-104), the PDQm Supplier (ITI-78, ITI-119), the
  PMIR Registry (ITI-93, ITI-94), the persisted cross-reference (identifier
  domains, one Source per domain, linking and merge), pseudonym minting (a
  UUID per pseudonymisation domain, for FerroEHR's `subject_namespaces`), the
  HL7 v2 PIX and PDQ transactions (ITI-8, 9, 21 and 22, with 44, 45, 46 and
  47 as the ITI TF-2 texts decide, once vendored), Manager-side BALP auditing
  through `ihe-atna`, and a published container image pinned by digest.

## Repo map

- `Cargo.toml`, `rust-toolchain.toml`, `deny.toml`, `clippy.toml`,
  `rustfmt.toml`: the workspace skeleton with the family lint set. The
  workspace has no member until the move.
- `docs/VERSIONS.md`: the pin matrix, held by `scripts/checks/versions.sh`.
  `docs/specs/` will hold the vendored corpora, each with a `PROVENANCE.md`.
- `scripts/gh/`: the tracker helpers (`fields.sh`, `rel.sh`, `labels.sh`,
  `watch-prs.sh`, `changelog-merge.sh`, and `project.sh`, a stub: there is no
  roadmap board). `scripts/checks/`: the guards (comment style, versions and
  licence headers, file length, `TODO` pointers, the crate version, the
  changelog, the contribution licence). `scripts/release/`: the milestone
  check and close and the crates.io publish. Every script has a
  `--self-test`, and CI runs them all.
- `.github/`: CI with the single required `conclusion` check, CodeQL,
  Scorecard, Dependabot, the release lane (`release.yml`,
  `release-crate.yml`) and `publish-crates.yml` through Trusted Publishing,
  the templates.
- `.claude/`: the working discipline (rules, agents, skills, hooks, memory).

## Issue workflow (the loop)

The tracker is GitHub Issues; the open issue list is the worklist
(`.claude/rules/issue-workflow.md`). Type, priority and effort are GitHub's
native issue type and the organisation's `Priority` and `Effort` fields, set
with `scripts/gh/fields.sh`; every issue is filed with `scripts/gh/fields.sh
new`. A Task carries exactly one work-kind label. Read an issue with `gh
issue view <n> --json title,body,comments`, never `--comments`. Milestones
are releases, stepping by 0.0.x. Native sub-issue and dependency edges are
set only with `scripts/gh/rel.sh` (`.claude/rules/issue-relationships.md`).
There is no roadmap board (`.claude/rules/project-board.md`). A PR declares
`Closes #N`, one keyword per issue, and is armed with `gh pr merge <n>
--auto`. New work found en route is filed and fixed before the next unit
starts.

## Model orchestration (workflows and subagents)

**When the session runs on Fable 5 (effort `high`), Fable is the orchestrator,
not the implementer.** Fable plans, coordinates, reviews, and does the taste-
and intelligence-heavy work itself; it fans implementation out to subagents
via the `Agent` tool (`model: 'opus'`, the newest Opus; the
`.claude/agents/*` definitions with `model: opus` pick it up) or a `Workflow`
(per-agent `model`). The main loop does not auto-delegate, so this section is
the standing instruction that it should.

| model     | cost | intelligence | taste |
|-----------|------|--------------|-------|
| fable-5   | 2    | 9            | 9     |
| opus-5    | 4    | 8            | 8     |

Only these two models are used. Sonnet and Haiku are not used in this
project; never pass them to `Agent` or `Workflow`.

- **Orchestrator (Fable 5, high):** owns the issue loop, the design
  decisions, spec-conformance judgement, and the hard bespoke logic (the
  linking and merge rules, the pseudonym model, the transaction semantics).
- **Delegate to Opus 5 subagents:** bulk implementation on a clear spec,
  file-heavy investigation and codebase analysis. At most 5 implementation
  workers at once. Prefer one worker with the full spec over splitting a
  coherent task. Tell workers not to spawn their own subagents, to write
  scratch files only under their own scratchpad subfolder, and never to wait
  with a self-matching `pgrep -f`.
- **Reviews:** spec questions go to `spec-researcher`; bounded
  implementation to `implementer`. Both are handed the governing spec
  sections in the prompt.

Discipline is unchanged for subagents: they obey the hard rules below.

## IMPORTANT hard rules

- **The specification text is the oracle**: the IHE ITI Technical Framework
  (Volumes 1 and 2), the IHE PIXm, PDQm and PMIR implementation guides, HL7
  FHIR R4, and the HL7 v2 PIX and PDQ transactions where in scope, as
  vendored under `docs/specs/`. Read the governing section before
  implementing or reviewing, and cite it in every `// NOTE:`; never resolve a
  question from memory or from another implementation. Where none governs a
  decision, say so: "no specification governs this: our own design"
  (`.claude/rules/spec-adherence.md`).
- **No identifying data leaks**: no demographics, identifier, pseudonym
  mapping, token or password in a log line, a `Debug` rendering or an error
  message. No patient data in a fixture, ever.
- **Never send the user's email address to any service**, a `User-Agent`
  header included.
- **Never hand-edit a `// @generated` file**, here or in a dependency
  (`.claude/rules/codegen.md`).
- **Comments follow RFC 505 and RFC 1574 with hard budgets**: line comments
  only, `// TODO(#NNNN):` naming its issue, `// NOTE:` as a citation and one
  sentence (`.claude/rules/comments.md`, enforced by
  `scripts/checks/comment-style.sh`).
- **Prose follows `.claude/rules/writing-style.md`**: no em dashes, no "not
  X but Y", no decorative triads, no filler buzzwords.
- **Branches use conventional types** (`feat/`, `fix/`, `chore/`, `docs/`,
  `refactor/`, `perf/`, `test/`, `ci/`, `build/`, `release/`) as
  `<type>/<kebab-case-slug>`, in a worktree under `.claude/worktrees/`. Every
  commit is signed; `main` takes changes only through pull requests and the
  merge queue. Never force-push `main`.
- **NEVER add AI or Claude attribution** to a commit, PR, issue, comment, or
  code comment: no `Co-Authored-By`, no "Generated with", no emoji footer,
  ever. A `PreToolUse` hook blocks a commit or PR command carrying one.
- **Keep the changelog.** `CHANGELOG.md` follows Keep a Changelog 1.1.0:
  every change with user-visible effect adds an entry under `[Unreleased]`
  in the same PR (`scripts/checks/changelog-guard.sh`).
- **Never weaken, skip, or delete a test** to make a build pass
  (`.claude/rules/testing.md`).
- **Record progress on the tracker and commit before ending a session.**
- **Remove a worktree, and run `cargo clean` in it, once its PR merges.**

## Licence

The project's own code and text are under the **Business Source License 1.1**
(`LICENSE`, `NOTICE`), Licensor Cadasto B.V. (info@cadasto.com), Change
License Apache 2.0 four years after each version. Every first-party file
carries `SPDX-FileCopyrightText: Cadasto B.V.` and
`SPDX-License-Identifier: BUSL-1.1` in its header, held by
`scripts/checks/versions.sh`. A contribution is licensed under the same
terms (CONTRIBUTING.md § Licensing of contributions); the pull request
checkbox records it and the `contribution-licence` job enforces it. Vendored
specifications keep their upstream terms, recorded in a `PROVENANCE.md`
beside each tree.

## Working discipline (`.claude/`)

- Rules: `writing-style`, `rust-style`, `reliability`, `comments`, `testing`,
  `spec-adherence`, `vendored-inputs`, `codegen`, `ci-cd`,
  `crates-publishing`, `issue-workflow`, `issue-relationships`,
  `project-board`.
- Agents: `spec-researcher`, `implementer` (both on Opus 5).
- Skills: `/spec-lookup`, `/next-task`, `/phase-done`, `/phase-status`.
- Hooks: `no_attribution_guard.sh`, `block_dangerous.sh` (with
  `--self-test`), `rust_fmt_clippy.sh`, `session_start_issues.sh`.
- Memory: `.claude/memory/MEMORY.md` is the index; every learning is a
  tracked file there.

## Sibling projects

All under FerroHEALTH, read-only prior art from here: read their code, rules
and history freely, and change them only in their own checkout, branch and
pull request under their own `CLAUDE.md`.

- **FerroFED** (`../FerroFED`): the federation gateway, where `ihe-iti` and
  `nl-generic-functions` were built first; it consumes them from here.
- **FerroEHR** (`../FerroEHR`): the openEHR CDR, which resolves `ehr_id`
  through FerroPIX.
- **FerroBRIDGE** (`../FerroBRIDGE`): the openEHR, FHIR and OMOP bridge, home
  of `fhir-types`, `hl7v2-types` and the MLLP codec and parser FerroPIX
  reuses.
- **FerroTERM** (`../FerroTERM`): the FHIR terminology server.
- **FerroSMART** (`../FerroSMART`): the shared SMART and OAuth 2.0 crate,
  `ferrohealth-smart`.
- **FerroAUDIT**: the shared audit crate, `ihe-atna` (ATNA and BALP).
- **FerroUI** (`../FerroUI`): the shared console component library, should
  FerroPIX ever get a console.

## References

- IHE ITI Technical Framework: <https://profiles.ihe.net/ITI/TF/index.html>
- IHE PIXm: <https://profiles.ihe.net/ITI/PIXm/>
- IHE PDQm: <https://profiles.ihe.net/ITI/PDQm/>
- IHE PMIR: <https://profiles.ihe.net/ITI/PMIR/>
- HL7 FHIR R4: <https://hl7.org/fhir/R4/>
- The tracker: `gh issue list --state open`.
