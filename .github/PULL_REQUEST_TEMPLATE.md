<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

## What changed and why

<!-- Describe the change and the reason for it. Name what a caller must change if an interface breaks. -->

Closes #NNN

## Licensing of contributions

- [ ] I accept the terms in [CONTRIBUTING.md § Licensing of contributions](../CONTRIBUTING.md#licensing-of-contributions): I have the right to submit this work, I license it under the project licence of the version it lands in, and I grant the Licensor the relicensing right stated there.

## Checklist

- [ ] Every conformance decision cites the specification (the IHE ITI Technical Framework, PIXm, PDQm, HL7 FHIR R4, HL7 v2), never memory and never another implementation.
- [ ] Rust gates pass, once the workspace has a member: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` with and without `--all-features`, `cargo nextest run --workspace --locked`, `cargo doc` with `RUSTDOCFLAGS=-D warnings`, and `cargo deny check`.
- [ ] Every refusal the change adds has an asserted negative test, and no test was weakened to pass.
- [ ] Shell and workflow files are clean: `shellcheck --severity=style`, `actionlint`, `zizmor --min-severity=low .github/`, and the self-test of every script the change touches.
- [ ] `CHANGELOG.md` has an `[Unreleased]` entry, or the change carries the `no-changelog` label.
- [ ] No patient data, real identifier, key, token or password in a fixture, test or example.
- [ ] Every commit is signed.
- [ ] No AI or assistant attribution anywhere in the commits or this PR.
