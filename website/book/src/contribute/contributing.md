<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Contributing

The tracker is the worklist: every change answers an issue of
[FerroHEALTH/FerroPIX](https://github.com/FerroHEALTH/FerroPIX/issues), and
each milestone is a release. Read
[CONTRIBUTING.md](https://github.com/FerroHEALTH/FerroPIX/blob/main/CONTRIBUTING.md)
before a first change; this page is the short form.

## Build and test

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-features --locked
cargo test --doc --workspace --all-features --locked
cargo deny check
```

A change to a workflow or a shell script also runs the linters CI runs
(`zizmor`, `actionlint`, `shellcheck --severity=style`) and the
`--self-test` of every script it touches. To build this book and the landing
page as the site serves them, install the documentation toolchain in
[the pin matrix](../evaluate/conformance.md) and run:

```sh
scripts/site/assemble.sh _site
scripts/checks/site-links.sh
```

## What a change carries

- **The specification it follows.** The IHE ITI Technical Framework, the
  PIXm, PDQm and PMIR guides, HL7 FHIR R4 and HL7 v2 decide every
  transaction. A change cites the section, and where none governs a choice it
  says so.
- **A changelog entry** under `[Unreleased]` in `CHANGELOG.md` for every
  change a user can see.
- **The book page it changes,** in the same pull request
  ([Keeping this book current](keeping-the-book-current.md)).
- **A crate version bump** when it changes what a published crate ships.
- **Signed commits** on a conventional branch (`feat/`, `fix/`, `docs/` and
  the rest), merged through the merge queue. Commit messages describe only
  the change.

## Licensing of contributions

A contribution is licensed under the Business Source License 1.1 with the
rest of the repository, and grants the Licensor the relicensing right in
[CONTRIBUTING.md](https://github.com/FerroHEALTH/FerroPIX/blob/main/CONTRIBUTING.md#licensing-of-contributions).
The pull request template's checkbox records it.
