<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Conformance and specification pins

The specifications are the authority for every transaction, parameter,
status and acknowledgment FerroPIX handles: the IHE ITI Technical Framework
(Volumes 1 and 2), the IHE PIXm, PDQm and PMIR implementation guides, HL7
FHIR R4, and the HL7 v2 PIX and PDQ transactions. Where none of them
governs a decision, the code says so and names the decision as FerroPIX's
own design.

## The specification versions

These are the versions the crates implement. The table is read from the
repository's pin matrix, `docs/VERSIONS.md`, when the book is built, so it
cannot disagree with it.

{{#include ../../../../docs/VERSIONS.md:specifications}}

## The toolchain

{{#include ../../../../docs/VERSIONS.md:toolchain}}

The minimum supported Rust version is one minor release below the toolchain
the repository builds with, the FerroHEALTH family's rule.

## How conformance is held

- **Every citation is checked against the vendored text.** The
  specification texts the crates read are vendored under `docs/specs/` at
  pinned versions, each with a `PROVENANCE.md`, and the `ihe-citations`
  check in CI holds every IHE section the tree cites to the vendored text of
  that profile and version. The ITI TF-2 texts of the HL7 v2 transactions
  (ITI-8, ITI-9, ITI-10, ITI-21 and ITI-22) reproduce HL7 tables whose
  rights HL7 reserves, so they are pinned by checksum and fetched into a
  local cache by the vendor script, never committed: a citation of one is
  checked when a local run has fetched it, and counted otherwise.
- **Each answer is held to its profile.** The clients refuse an answer that
  departs from the transaction: a PIXm `$ihe-pix` answer is held to its
  `OperationDefinition`, a PDQm search to the Query Patient Resource Response
  Message profile, an ITI-93 feed message to the PMIR profiles. The crate
  pages say what each client checks.
- **An unknown patient is never an outage.** A `404` reads as "patient
  unknown" only when its `OperationOutcome` carries a `not-found` issue, as
  PIXm and PDQm define it; every other failure is a typed error.
- **Each feature builds and tests on its own.** CI lints and tests every
  feature of both crates alone and with its audit feature, as well as all of
  them together.

## The conformance gate

The corpus tests of `ihe-iti` read and write every example the vendored
implementation guides ship, and `scripts/checks/conformance.sh` holds each
corpus to its committed pass list. A failing example stays in the list as
failing, with its reason, so the count shows what is not met. Each corpus
renders the shields.io badge the README shows.

| Corpus | What each example goes through |
|---|---|
| [PIXm 3.1.0](https://github.com/FerroHEALTH/FerroPIX/blob/main/conformance/pixm/pass-list.txt) | An ITI-83 request read by the Manager in JSON and XML, an answer read by the client and written by the Manager, the not-found answer both ways, and each Patient read as the ITI-104 conditional update its first identifier names. |
| [PDQm 3.2.0](https://github.com/FerroHEALTH/FerroPIX/blob/main/conformance/pdqm/pass-list.txt) | An ITI-78 answer and each ITI-119 answer read by the client and written by the Supplier, each `$match` input read by the Supplier, and each Patient read from a search answer and written as a retrieve. |
| [PMIR 1.6.0](https://github.com/FerroHEALTH/FerroPIX/blob/main/conformance/pmir/pass-list.txt) | Each ITI-93 message read by the Consumer as the changes it names, the response written by the Consumer, the ITI-94 request written and a Subscription's status read by the subscriber. |
| [Audit records](https://github.com/FerroHEALTH/FerroPIX/blob/main/conformance/audit/pass-list.txt) | Each `AuditEvent` example against the record `ihe-iti` writes for that exchange, held to the example's audit profile, the BALP pattern beneath it and the example itself. The Manager and Supplier records are written by the server, so they count as failing here. |

Run `scripts/checks/conformance.sh` to compare, `--update` to rewrite the
lists, the badge files and the README badge block after a change, and
`--check` for what CI runs: a listed pass that stops passing fails every
mode, and under `--check` so does a new pass, a moved total or a changed
reason.

There is no conformance statement for the server yet: it is part of the
[v0.0.3 milestone](https://github.com/FerroHEALTH/FerroPIX/milestone/3),
which is planned.

## The published texts

- IHE ITI Technical Framework: <https://profiles.ihe.net/ITI/TF/index.html>
- IHE PIXm: <https://profiles.ihe.net/ITI/PIXm/>
- IHE PDQm: <https://profiles.ihe.net/ITI/PDQm/>
- IHE PMIR: <https://profiles.ihe.net/ITI/PMIR/>
- IHE mCSD: <https://profiles.ihe.net/ITI/mCSD/>
- HL7 FHIR R4: <https://hl7.org/fhir/R4/>
- The Netherlands Generic Functions implementation guide:
  <https://build.fhir.org/ig/nuts-foundation/nl-generic-functions-ig/>
