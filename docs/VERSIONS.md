<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Pinned version matrix

This file is the single source of truth for every version pin in FerroPIX.
When it and a file that repeats a pin disagree, that is drift. Fix the
disagreement; never let either side win silently.
`scripts/checks/versions.sh` reads the toolchain rows back against
`rust-toolchain.toml` and the root `Cargo.toml`, and every vendored tree
under `docs/specs/` against a row of the corpus table.

No specification governs this file; it is FerroPIX's own design.

## Toolchain

| Item | Pin | Repeated in |
|---|---|---|
| Rust toolchain | `1.98.1` | `rust-toolchain.toml` |
| Minimum supported Rust version | `1.97` | `Cargo.toml` `[workspace.package] rust-version` |
| Edition | `2024` | `Cargo.toml` `[workspace.package] edition`, `rustfmt.toml` |

## Specifications

These are the versions FerroFED vendors today for the identity work that
moves here (FerroFED's `docs/VERSIONS.md`, read on 2026-10-20). They become
pins of this repository when the move vendors their corpora under
`docs/specs/`, each with a `scripts/vendor/*.sh` and a `PROVENANCE.md`.

| Item | Version | Transactions |
|---|---|---|
| IHE PIXm | `ihe.iti.pixm` 3.1.0 | ITI-83 Mobile Patient Identifier Cross-reference Query, ITI-104 Patient Identity Feed FHIR |
| IHE PDQm | `ihe.iti.pdqm` 3.2.0 | ITI-78 Mobile Patient Demographics Query, ITI-119 Patient Demographics Match |
| IHE PMIR | `ihe.iti.pmir` 1.6.0 | ITI-93 Mobile Patient Identity Feed, ITI-94 Subscribe to Patient Updates |
| IHE mCSD | `ihe.iti.mcsd` 4.0.0 | ITI-90, ITI-91 (a feature of `ihe-iti`) |
| IHE XCPD | ITI Technical Framework Vol 2 Rev 20.1 (2024-12-12), no FHIR package | ITI-55 (a feature of `ihe-iti`) |
| IHE IUA | ITI Technical Framework Supplement Rev 2.5 (2026-06-18) | ITI-71, ITI-72 (a feature of `ihe-iti`) |
| Netherlands Generic Functions | `fhir.nl.gf` 0.3.0 | the functions of `nl-generic-functions` |
| IHE ITI Technical Framework | Volumes 1 and 2, the revision the move pins | PIX and PDQ: ITI-8, ITI-9, ITI-10, ITI-21 and ITI-22, and ITI-44 to ITI-47 as the TF-2 texts decide, with the actors and options of each |
| HL7 FHIR | R4, 4.0.1 | the resources and the RESTful API PIXm and PDQm are profiled on |
| HL7 v2 | the version each PIX or PDQ transaction names in ITI TF-2 | the HL7 v2 PIX and PDQ transactions, where in scope |

## Corpora

A corpus is pinned by commit, immutable publication or checksum, never by a
moving tag or a `latest` URL, and vendored by a committed `scripts/vendor/*.sh`
with a `PROVENANCE.md`. No corpus is vendored yet.

| Item | Pin | Repeated in |
|---|---|---|
