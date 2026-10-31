<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Contributing to FerroPIX

FerroPIX is the FerroHEALTH family's Master Patient Index over IHE PIX,
PIXm, PDQ and PDQm. The working discipline is [`CLAUDE.md`](CLAUDE.md) and
the plan is the tracker of this repository, with one milestone per release.
Read both before making a change.

## Build and test

Once the workspace has a member (the first crates arrive with the move from
FerroFED):

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --locked
cargo nextest run --workspace --all-features --locked
cargo test --doc --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --all-features --no-deps --document-private-items
cargo deny check
```

A change to a workflow or a shell script also needs the linters CI runs,
and the self-test of every script it touches:

```
zizmor --min-severity=low .github/
actionlint
shellcheck --severity=style <the shell files you touched>
bash scripts/checks/<guard>.sh --self-test
```

## The specifications are the authority

The IHE ITI Technical Framework, the IHE PIXm, PDQm and PMIR implementation
guides, HL7 FHIR R4 and the HL7 v2 PIX and PDQ transactions decide every
transaction, parameter, status and acknowledgment; the docs.rs pages of the
pinned crates decide how the tools are used. Cite the section in the pull
request and in each `// NOTE:`. Where none governs a choice, say so: "no
specification governs this: our own design".

## Branches and commits

- Branch from `main` with a conventional-type name: `feat/<slug>`,
  `fix/<slug>`, `chore/<slug>`, `docs/<slug>`, `refactor/<slug>`,
  `perf/<slug>`, `test/<slug>`, `ci/<slug>`, `build/<slug>`,
  `release/<slug>`.
- Commit messages describe only the change. Do **not** add AI or assistant
  attribution, co-author trailers, or "generated with" lines anywhere.
- **Sign your commits.** `main` refuses an unsigned commit and takes changes
  only through pull requests and the merge queue.
- Every first-party file carries the SPDX header
  (`SPDX-FileCopyrightText: Cadasto B.V.`,
  `SPDX-License-Identifier: BUSL-1.1`).

## Pull requests

- Declare `Closes #N` for each tracker issue the pull request answers, one
  keyword per issue.
- Never weaken, skip, or delete a test to make a build pass; every refusal
  you add has an asserted negative test.
- Add a changelog fragment, `changelog.d/<issue>-<kebab-slug>.<section>.md`
  (`changelog.d/README.md`), for any change a caller sees, and name what a
  caller must change when an interface breaks. Never edit `[Unreleased]` in
  `CHANGELOG.md`: the release cut assembles the fragments.
- Never put patient data or a real identifier in a fixture, a test or an
  example.
- Write prose to `.claude/rules/writing-style.md`.

## Using AI tools

You may. If a contribution has AI-generated content, say so in the pull
request description: which tool, and what it did. Disclosure lives in the
description, never in a commit trailer. You remain responsible for the
submission in full.

## Licensing of contributions

FerroPIX's own code is licensed under the Business Source License 1.1
([`LICENSE`](LICENSE)). By opening a pull request you:

1. certify that you wrote it, or otherwise have the right to submit it under
   these terms;
2. license it under the Business Source License 1.1 as applied to the version it
   lands in, including that version's Change License, so it becomes Apache 2.0
   with the rest of that version; and
3. grant the Licensor named in `LICENSE` a perpetual, irrevocable, worldwide,
   royalty-free, transferable right to use, reproduce, modify, distribute,
   sublicense and relicense the contribution as part of the Licensed Work under
   any terms, including commercial licences.

You keep your copyright. Point 3 is what lets the Licensed Work stay one work
with one licensor: a commercial licence, a change of the licence parameters, or a
transfer of the project can then cover every line, not only the maintainer's own.
There is no separate agreement to sign and no box to tick: opening the pull
request is your acceptance of these terms.

## Security

Report a vulnerability privately through the address in
[`SECURITY.md`](SECURITY.md), never in a public issue.
