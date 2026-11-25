<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Memory index

- [CI under 20 minutes](ci-under-20-minutes.md): every pull-request and
  merge-group run of `ci.yml` under 20 minutes, queue included; the
  performance gate runs on the pull request and on `main`, never in the
  merge queue; owner 2026-11-09
- [Family architecture](family-architecture.md): the FerroHEALTH family
  is five separate servers that reach each other only over published
  protocols, plus shared crates for cross-cutting work; nothing is
  hand-written that a shared crate or another product provides; owner
  2026-10-25
- [Memory lives in the repo](memory-lives-in-repo.md): every learning is a
  tracked file here, never a per-user note
- [What FerroPIX is](product-scope.md): the family's Master Patient Index
  and the one product that owns identity and the IHE profiles, owning
  `ihe-iti` and `nl-generic-functions`, FHIR and HL7 v2 together; owner
  2026-10-20 and 2026-10-25
- [Built in FerroFED first, then moved](build-in-fed-first.md): history; the
  identity protocols moved here in v0.0.2 (FerroFED#367) and the rule is
  retired, since identity is FerroPIX's lane alone
- [Corpora move with the crates](corpora-move-with-crates.md): every corpus
  the moved crates read comes here with its script and provenance; each
  FerroFED swap PR removes the copies nothing there reads; owner 2026-10-20
- [ihe-iti audit feature](ihe-iti-audit-feature.md): each profile's audit
  record stays in ihe-iti behind `balp` and `atna`, on ihe-atna's trail and
  user model
- [Licence: BUSL 1.1](license-busl.md): the FerroHEALTH terms, Cadasto B.V.
  the Licensor, Apache 2.0 four years after each version
- [No AI attribution](no-ai-attribution.md): never, anywhere; the hook
  blocks it
- [Support policy](support-policy.md): only the newest release, with the CRA
  five-year fix commitment
- [Signed commits and the merge queue](merge-queue-signed.md): `main` takes
  only signed commits through pull requests and the merge queue; no
  commit-message line starts with "-" or a git header word (2026-11-09)
- [Auto-merge](pr-auto-merge.md): arm every pull request with
  `gh pr merge <n> --auto`
- [Two version lines](two-version-lines.md): the product version is the
  workspace and the tag; each published crate keeps its own line
- [Release tag](release-tag.md): signed on the merge commit, pushed from the
  session; the session approves the crates-io gate itself (owner 2026-11-03)
- [Release closes milestone](release-closes-milestone.md): release.yml
  refuses a tag whose milestone is not empty and closes it after every leg
- [Worker scratch subfolders](worker-scratch-subfolders.md): scratch files
  under the worker's own subfolder; re-read every PR body after setting it
- [Remove worktrees after merge](remove-worktrees-after-merge.md): every
  checkout under `.claude/worktrees/`, removed with `cargo clean` once merged,
  in every family repository
- [Worker wait loops](worker-wait-loops.md): no self-matching `pgrep -f`;
  poll at most every 60 seconds
- [FerroUI lockstep](ferroui-lockstep.md): the operator console
  `ferropix-console` (owner 2026-10-21, built on FerroUI 0.3.2 in v0.0.3) is
  in the library's FerroPIX brand, server-rendered like every console, moves in
  lockstep with the others, and every gap is filed and built in FerroUI
- [The book stays current](book-same-pr.md): ferropix.eu documents only what
  the code does, changes in the same PR, links the tracker for the planned,
  never types a moving version; owner 2026-10-21
- [Helm and Kubernetes in Podman](helm-kubernetes-in-podman.md): owner 2026-10-25, test charts locally on kind over Podman; install helm when missing; never build images locally
- [Reactive console UI](reactive-console-ui.md): superseded 2026-10-29 by FerroUI #510; every console is drawn on the server, a navigation is a document load, an in-page change a FerroUI module, no WebAssembly

## Family rules (carried in every FerroHEALTH repository, 2026-11-09)

- [FerroHEALTH not in production](ferrohealth-not-in-production.md): no product runs in production, so a wrong design is refactored or rewritten outright with no compatibility layer; owner 2026-11-19
- [New design, no legacy](new-design-no-legacy.md): a breaking release builds the new design in full and keeps nothing of the old; never relay a worker's kept-for-compatibility choice as a question; owner 2026-11-14
- [Findings drive redesign](findings-drive-redesign.md): a scenario or test the design cannot handle is a design finding, never worked around in a fixture or probe; owner 2026-11-14
- [Verify old issues first](verify-before-implementing-old-issues.md): check an older issue against main and the current spec before building it; close what is done or obsolete with evidence; owner 2026-11-16
- [Orchestrator files cross-repo gaps](orchestrator-files-cross-repo-gaps.md): the orchestrator files every gap a worker finds in another family repo, without asking the owner; owner 2026-11-10
- [Backlog fix comments](backlog-fix-comments.md): when a fix for a Cadasto/FerroHEALTH-backlog issue merges, comment there and set it to In review; owner 2026-11-17
- [Efficiency first](efficiency-first-rust.md): stable, fast, low on storage, CPU and RAM; no needless clones, allocations or forwarding layers; owner 2026-11-16
- [Spec over test suites](spec-over-test-suites.md): external conformance suites are regression signals, never the oracle; owner 2026-11-16
- [Quick start from the release download](quickstart-from-release-download.md): one self-contained compose.yaml per product as a release asset, fetched with curl; no clone, no bundle; owner 2026-11-10
- [Owner voice for outward text](owner-voice-outward-text.md): text the owner sends as themselves is plain and short, few headings or bullets; owner 2026-11-14
- [Owner work style](owner-work-style.md): research first and evidence-based, foundational decisions confirmed before code, defer nothing
- [PostgreSQL 19](postgresql-19.md): every product with a database runs, tests against and documents PostgreSQL 19; 18 and older dropped; owner 2026-11-21
- [No compatibility shims](no-compat-shims.md): refactors break APIs freely and leave no deprecated item, alias, re-export or stub; a removal refusal lives ONE release, then is deleted fully; no legacy support ever; owner 2026-11-01, 2026-11-09
- [No half solutions](no-half-solutions.md): build what the issue asks the proper way and validate the need against its consumer first; a breaking change moves the minor version, never shape a design to dodge semver; owner 2026-11-08
- [EHDS above openEHR](ehds-above-openehr.md): where Regulation (EU) 2025/327 conflicts with a standard's text the Regulation wins; never revert EHDS behaviour to pass a conformance row; owner 2026-11-09
- [Terminology via the server](terminology-via-server.md): every production value map goes through a FHIR terminology server over ferrobridge-term; embedded or offline maps are test-only; owner 2026-11-09
- [Workers on Opus only](workers-on-opus-only.md): every subagent is model opus; never Fable, Sonnet or Haiku as a worker; owner 2026-10-30
- [Ten worker slots](worker-cap-ten.md): up to 10 implementation workers at once across the family since 2026-11-03, replacing the cap of 5
- [No workflows for workers](no-workflows-for-workers.md): start workers with the Agent tool only, since a workflow agent cannot be messaged; owner 2026-11-01
- [Workers never idle on the merge queue](workers-never-idle-on-merge-queue.md): start the next independent issue in a fresh worktree while a PR waits in the queue; owner 2026-11-03
- [Workers own every open PR](workers-own-every-open-pr.md): drive every PR you opened to merge; the orchestrator sweeps every open PR after each merge, red main first; owner 2026-11-01, 2026-11-03
- [PR backlog discipline](pr-backlog-discipline.md): several open PRs are fine if each merges cleanly; disjoint files, no stacked PRs, every PR tracked to its merge; owner 2026-10-17
- [Worktrees in the repo](worktrees-in-repo.md): every worktree at <repo>/.claude/worktrees/<name>, never a sibling folder or the scratchpad; owner 2026-10-21
- [Admin bypass stays](admin-bypass-stays.md): the org-admin bypass on main stays in every family repo; nobody merges through it; owner 2026-10-21
- [Decide, do not ask](decide-dont-ask.md): make tracker, design and release calls yourself and act; ask the owner only when a tool is refused; owner 2026-11-03
- [Autonomous loop](autonomous-loop.md): never stop and never wait idle; keep working the issues while PRs drain; owner 2026-10-17
- [Always a next milestone](next-milestone-always.md): every repo keeps a milestone with a due date after the active one; owner 2026-11-03
- [One milestone at a time](one-milestone-at-a-time.md): every worker on the current milestone until it is cut; en-route issues join it and are fixed first; owner 2026-10-19
- [Shared-crate blockers](shared-crate-blockers.md): every filed issue gets a milestone; a shared-crate defect goes in the owner repo's current milestone with a worker at once; no consumer workaround; owner 2026-11-07
- [Family toolchain](family-toolchain.md): newest stable Rust as toolchain, the toolchain minus one as MSRV; never lower an MSRV; owner 2026-10-21
- [Subagent reports go to a file](subagent-reports-to-file.md): a long agent report is written to the agent's scratch subfolder and read from there; 2026-09-12
- [Sibling projects](sibling-projects.md): the other family repositories are read-only prior art from here; a change there is made in its own checkout and PR
- [Lean comments, no history](lean-comments-no-history.md): comments rare and short, describing only the current code; a changed comment is rewritten whole, never extended with the old way; owner hard rule 2026-11-23
- [Family memories in every repo](family-memories-every-repo.md): a family-wide owner rule goes into every FerroHEALTH repo's `.claude/memory` through a merged PR; owner 2026-11-23
- [Mermaid diagrams](mermaid-diagrams.md): every diagram is a mermaid block, never ASCII art; render each fence before a PR; owner 2026-11-24
- [Merge queue is light](merge-queue-light.md): the merge-group run is the guards, rustfmt, cargo-deny and a unit-test smoke set; the full suite runs on the pull request and on main; owner 2026-11-25
